//! AU8: THE GIF WRITER: a shareable file with no add-on. GIF89a with one global
//! palette of at most 256 colours (median cut over a 5-6-5 histogram, no
//! dither), looping forever, flattened on white. A held drawing is one image
//! with a long delay. The file lands whole: it grows beside its name and
//! [`GifWriter::finish`] renames it, so a stopped render leaves nothing.
//!
//! The palette has to be in the file before the first image, so a job shows
//! the writer the frames [`sample_frames`] picks, then pushes every frame.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use weezl::BitOrder;
use weezl::encode::Encoder;

/// The most frames a palette is built from, so a long shot does not render
/// twice.
pub const PALETTE_FRAMES: u32 = 16;

/// The fewest hundredths a picture stays up. Viewers run anything shorter at
/// 10, so a GIF holds at most 50 pictures a second: past 50 fps some frames get
/// no image, the first and the last always do, and the running time stands.
/// Only the last picture rounds up to this, so a shot runs at most this much
/// over, except that two pictures can never run under twice this.
pub const MIN_DELAY_CS: u32 = 2;

const MAX_COLOURS: usize = 256;

/// `NETSCAPE2.0`, loop count 0: forever.
const LOOP_BLOCK: &[u8] = b"\x21\xFF\x0BNETSCAPE2.0\x03\x01\x00\x00\x00";

/// A histogram cell no pixel has asked about yet.
const NO_INDEX: u16 = u16::MAX;

/// The frames of `first..=last` a palette is built from: at most
/// [`PALETTE_FRAMES`], evenly spaced, the first and the last among them.
pub fn sample_frames(first: u32, last: u32) -> Vec<u32> {
    let span = last.saturating_sub(first) as u64;
    let n = (span + 1).min(PALETTE_FRAMES as u64);
    (0..n)
        .map(|i| first + (i * span / (n - 1).max(1)) as u32)
        .collect()
}

/// How long frame `index` of a shot at `fps` lasts, in hundredths of a second:
/// the running remainder `round(cum_frames × 100 / fps) - prev`, so the rounding
/// never adds up to drift.
pub fn delay_cs(index: u32, fps: u32) -> u32 {
    let fps = fps.max(1) as u64;
    let cum = |frames: u64| (frames * 200 + fps) / (2 * fps);
    (cum(index as u64 + 1) - cum(index as u64)) as u32
}

/// One cell of the 5-6-5 histogram. It sums its channels, so a cell holding
/// one colour gives that colour back exactly.
#[derive(Debug, Clone, Copy, Default)]
struct Cell {
    pixels: u64,
    sum: [u64; 3],
}

/// One straight RGBA pixel laid on white.
fn on_white(p: &[u8]) -> [u8; 3] {
    let a = p[3] as u32;
    [0, 1, 2].map(|c| ((p[c] as u32 * a + 255 * (255 - a) + 127) / 255) as u8)
}

fn cell_of([r, g, b]: [u8; 3]) -> usize {
    (r as usize >> 3) << 11 | (g as usize >> 2) << 5 | b as usize >> 3
}

/// A cell's place on `axis` (red, green, blue), on the same scale for all three.
fn place(cell: u16, axis: usize) -> u16 {
    match axis {
        0 => (cell >> 11) << 3,
        1 => ((cell >> 5) & 63) << 2,
        _ => (cell & 31) << 3,
    }
}

/// The cell's colour: what the samples put there, else its middle.
fn colour_of(cell: usize, hist: &[Cell]) -> [u8; 3] {
    let Cell { pixels, sum } = hist[cell];
    if pixels == 0 {
        let middle = [4, 2, 4];
        return [0, 1, 2].map(|axis| (place(cell as u16, axis) + middle[axis]) as u8);
    }
    sum.map(|s| ((s + pixels / 2) / pixels) as u8)
}

/// One box of the median cut.
struct Cube {
    cells: Vec<u16>,
    pixels: u64,
    /// The widest channel, and how wide (0: one cell, it cannot split).
    axis: usize,
    spread: u16,
}

impl Cube {
    fn new(cells: Vec<u16>, hist: &[Cell]) -> Self {
        let spread = |axis: usize| {
            let places = || cells.iter().map(|&c| place(c, axis));
            places().max().unwrap_or(0) - places().min().unwrap_or(0)
        };
        let axis = (0..3).max_by_key(|&a| spread(a)).unwrap_or(0);
        let spread = spread(axis);
        let pixels = cells.iter().map(|&c| hist[c as usize].pixels).sum();
        Self { cells, pixels, axis, spread }
    }

    /// At its pixel median, along its widest channel.
    fn split(mut self, hist: &[Cell]) -> (Cube, Cube) {
        let axis = self.axis;
        self.cells.sort_unstable_by_key(|&c| (place(c, axis), c));
        let mut seen = 0;
        let median = self.cells.iter().position(|&c| {
            seen += hist[c as usize].pixels;
            seen >= self.pixels / 2
        });
        let cut = median.map_or(1, |i| i + 1).clamp(1, self.cells.len() - 1);
        let high = self.cells.split_off(cut);
        (Cube::new(self.cells, hist), Cube::new(high, hist))
    }

    /// The mean of its pixels.
    fn colour(&self, hist: &[Cell]) -> [u8; 3] {
        let sum = self.cells.iter().fold([0u64; 3], |s, &c| {
            let cell = hist[c as usize].sum;
            [s[0] + cell[0], s[1] + cell[1], s[2] + cell[2]]
        });
        sum.map(|s| ((s + self.pixels / 2) / self.pixels.max(1)) as u8)
    }
}

/// Median cut: the box with the most pixels × the widest spread splits, until
/// there are 256 boxes or every box is one cell.
fn median_cut(hist: &[Cell]) -> Vec<[u8; 3]> {
    let all = (0..=u16::MAX).filter(|&c| hist[c as usize].pixels > 0);
    let mut cubes = vec![Cube::new(all.collect(), hist)];
    while cubes.len() < MAX_COLOURS {
        let heaviest = (0..cubes.len())
            .filter(|&i| cubes[i].spread > 0)
            .max_by_key(|&i| cubes[i].pixels * cubes[i].spread as u64);
        let Some(i) = heaviest else { break };
        let (low, high) = cubes.swap_remove(i).split(hist);
        cubes.push(low);
        cubes.push(high);
    }
    cubes.iter().map(|c| c.colour(hist)).collect()
}

/// What the header fixed: the palette, the LZW code size, and each cell's
/// palette index once a pixel has asked.
struct Table {
    palette: Vec<[u8; 3]>,
    code_size: u8,
    index: Vec<u16>,
}

impl Table {
    fn nearest(&mut self, cell: usize, hist: &[Cell]) -> u8 {
        if self.index[cell] == NO_INDEX {
            let c = colour_of(cell, hist);
            let off = |p: &[u8; 3]| -> i32 {
                (0..3).map(|i| (p[i] as i32 - c[i] as i32).pow(2)).sum()
            };
            let best = (0..self.palette.len()).min_by_key(|&i| off(&self.palette[i]));
            self.index[cell] = best.unwrap_or(0) as u16;
        }
        self.index[cell] as u8
    }
}

/// One GIF being written: [`Self::new`], [`Self::sample`] for the frames the
/// palette is built from, [`Self::push_frame`] for every frame in order,
/// [`Self::finish`]. Dropped before it finishes, it leaves no file.
pub struct GifWriter {
    path: PathBuf,
    part: PathBuf,
    /// None once the file has landed.
    out: Option<BufWriter<File>>,
    size: (u16, u16),
    hist: Vec<Cell>,
    table: Option<Table>,
    /// The picture in hand: its palette indices and how long it has lasted.
    /// It moves on when a different one arrives, so a hold is one image.
    held: Option<(Vec<u8>, u32)>,
    /// The image before it, not in the file yet, so the last frame can still
    /// join it. None while `held` is the file's first image.
    behind: Option<(Vec<u8>, u32)>,
    /// The last frame pushed, when it came too soon after `held` to get an
    /// image. If nothing follows it, [`Self::finish`] still ends the file on it.
    too_soon: Option<Vec<u8>>,
}

impl GifWriter {
    /// A `w × h` GIF to land at `path`; a size a GIF cannot hold is refused.
    pub fn new(path: &Path, w: u32, h: u32) -> Result<Self, String> {
        let size = u16::try_from(w).ok().zip(u16::try_from(h).ok());
        let Some(size) = size.filter(|&(w, h)| w > 0 && h > 0) else {
            return Err(format!("a GIF cannot be {w}×{h}"));
        };
        let mut part = path.as_os_str().to_owned();
        part.push(".part");
        let part = PathBuf::from(part);
        let file = File::create(&part).map_err(|e| format!("write {}: {e}", path.display()))?;
        Ok(Self {
            path: path.to_path_buf(),
            part,
            out: Some(BufWriter::new(file)),
            size,
            hist: vec![Cell::default(); 1 << 16],
            table: None,
            held: None,
            behind: None,
            too_soon: None,
        })
    }

    /// Show the palette a frame (straight RGBA, row by row). Every sample comes
    /// before the first [`Self::push_frame`]; with none, the first frame pushed
    /// is the sample.
    pub fn sample(&mut self, rgba: &[u8]) -> Result<(), String> {
        self.check(rgba)?;
        for p in rgba.chunks_exact(4) {
            let c = on_white(p);
            let cell = &mut self.hist[cell_of(c)];
            cell.pixels += 1;
            for i in 0..3 {
                cell.sum[i] += c[i] as u64;
            }
        }
        Ok(())
    }

    /// The next frame, lasting `delay_cs` hundredths. The same picture as the
    /// one in hand adds to its time, and so does any picture while the one in
    /// hand is under [`MIN_DELAY_CS`], which then gets no image unless it turns
    /// out to be the last frame.
    pub fn push_frame(&mut self, rgba: &[u8], delay_cs: u32) -> Result<(), String> {
        self.check(rgba)?;
        if self.table.is_none() {
            if self.hist.iter().all(|c| c.pixels == 0) {
                self.sample(rgba)?;
            }
            self.header()?;
        }
        let hist = &self.hist;
        let table = self.table.as_mut().expect("the header set it");
        let indices: Vec<u8> = rgba
            .chunks_exact(4)
            .map(|p| table.nearest(cell_of(on_white(p)), hist))
            .collect();
        match &mut self.held {
            Some((held, cs)) if *held == indices => {
                *cs += delay_cs;
                self.too_soon = None;
            }
            Some((_, cs)) if *cs < MIN_DELAY_CS => {
                *cs += delay_cs;
                self.too_soon = Some(indices);
            }
            _ => {
                self.too_soon = None;
                let shown = self.held.replace((indices, delay_cs));
                if let Some((image, cs)) = std::mem::replace(&mut self.behind, shown) {
                    self.write_image(&image, cs)?;
                }
            }
        }
        Ok(())
    }

    /// Write the last pictures and land the file. A file always ends on the
    /// last frame: one that came too soon takes the place and the time of the
    /// picture in hand, unless that picture is the first frame, which stays too.
    pub fn finish(mut self) -> Result<(), String> {
        let Some((held, cs)) = self.held.take() else {
            return Err(format!("{}: a GIF with no frame", self.path.display()));
        };
        let end = match (self.behind.take(), self.too_soon.take()) {
            (behind, None) => behind.into_iter().chain([(held, cs)]).collect(),
            (None, Some(last)) => {
                vec![(held, MIN_DELAY_CS), (last, cs.saturating_sub(MIN_DELAY_CS))]
            }
            (Some((image, shown)), Some(last)) if image == last => vec![(last, shown + cs)],
            (Some(behind), Some(last)) => vec![behind, (last, cs)],
        };
        for (image, cs) in end {
            self.write_image(&image, cs)?;
        }
        self.put(&[0x3B])?;
        let flushed = self.out.take().map_or(Ok(()), |mut out| out.flush());
        flushed
            .and_then(|()| std::fs::rename(&self.part, &self.path))
            .map_err(|e| {
                let _ = std::fs::remove_file(&self.part);
                format!("write {}: {e}", self.path.display())
            })
    }

    fn check(&self, rgba: &[u8]) -> Result<(), String> {
        let (w, h) = self.size;
        match rgba.len() == w as usize * h as usize * 4 {
            true => Ok(()),
            false => Err(format!("a frame of {} bytes in a {w}×{h} GIF", rgba.len())),
        }
    }

    fn put(&mut self, bytes: &[u8]) -> Result<(), String> {
        let out = self.out.as_mut().expect("open until it lands");
        out.write_all(bytes)
            .map_err(|e| format!("write {}: {e}", self.path.display()))
    }

    /// Everything before the first image: the palette is fixed here.
    fn header(&mut self) -> Result<(), String> {
        let palette = median_cut(&self.hist);
        // The table holds a power of two, at least 2; LZW starts at 2 bits.
        let bits = palette.len().next_power_of_two().max(2).trailing_zeros();
        let (w, h) = self.size;
        let mut head = b"GIF89a".to_vec();
        head.extend_from_slice(&w.to_le_bytes());
        head.extend_from_slice(&h.to_le_bytes());
        // A global table of 8-bit colour; background 0; no aspect.
        head.extend_from_slice(&[0xF0 | (bits - 1) as u8, 0, 0]);
        head.extend(palette.iter().flatten());
        head.resize(13 + (3 << bits), 0);
        head.extend_from_slice(LOOP_BLOCK);
        self.put(&head)?;
        self.table = Some(Table {
            palette,
            code_size: bits.max(2) as u8,
            index: vec![NO_INDEX; 1 << 16],
        });
        Ok(())
    }

    /// One picture as an image: LZW in sub-blocks of 255, repeated when its
    /// time will not fit one delay field.
    fn write_image(&mut self, indices: &[u8], cs: u32) -> Result<(), String> {
        let code_size = self.table.as_ref().expect("the header set it").code_size;
        let lzw = Encoder::new(BitOrder::Lsb, code_size)
            .encode(indices)
            .map_err(|e| format!("write {}: {e}", self.path.display()))?;
        let (w, h) = self.size;
        let mut image = vec![0x2C, 0, 0, 0, 0];
        image.extend_from_slice(&w.to_le_bytes());
        image.extend_from_slice(&h.to_le_bytes());
        image.extend_from_slice(&[0, code_size]);
        for block in lzw.chunks(255) {
            image.push(block.len() as u8);
            image.extend_from_slice(block);
        }
        image.push(0);
        let mut left = cs.max(MIN_DELAY_CS);
        while left > 0 {
            let delay = left.min(u16::MAX as u32) as u16;
            left -= delay as u32;
            let [lo, hi] = delay.to_le_bytes();
            // Graphic control: keep this picture, no transparent colour.
            self.put(&[0x21, 0xF9, 4, 0x04, lo, hi, 0, 0])?;
            self.put(&image)?;
        }
        Ok(())
    }
}

impl Drop for GifWriter {
    /// Dropped before it landed (a stopped render): the part file goes.
    fn drop(&mut self) {
        if self.out.take().is_some() {
            let _ = std::fs::remove_file(&self.part);
        }
    }
}
