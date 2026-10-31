//! AU8: a small GIF decoder for the tests, over weezl. It reads the file block
//! by block and refuses whatever a viewer would trip on, so a passing test
//! means a whole file. `gif_tests.rs` and the app's tests share this one.

#![allow(dead_code)]

use weezl::BitOrder;
use weezl::decode::Decoder;

/// One image of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// In hundredths of a second.
    pub delay_cs: u16,
    /// Palette indices, row by row.
    pub indices: Vec<u8>,
    /// The same pixels as colours.
    pub rgb: Vec<[u8; 3]>,
}

#[derive(Debug, Clone)]
pub struct Gif {
    pub size: (u16, u16),
    pub palette: Vec<[u8; 3]>,
    /// The NETSCAPE2.0 block's count (0 = forever).
    pub loops: Option<u16>,
    pub frames: Vec<Frame>,
    /// Every image's sub-block lengths, in order.
    pub sub_blocks: Vec<Vec<usize>>,
}

impl Gif {
    pub fn delays(&self) -> Vec<u16> {
        self.frames.iter().map(|f| f.delay_cs).collect()
    }

    pub fn duration_cs(&self) -> u32 {
        self.frames.iter().map(|f| f.delay_cs as u32).sum()
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at + n;
        let got = self
            .bytes
            .get(self.at..end)
            .ok_or_else(|| format!("the file ends inside a block at byte {}", self.at))?;
        self.at = end;
        Ok(got)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, String> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn expect(&mut self, want: &[u8], what: &str) -> Result<(), String> {
        match self.take(want.len())? {
            got if got == want => Ok(()),
            got => Err(format!("{what}: {got:?}, not {want:?}")),
        }
    }
}

/// Decode a whole file. Err names the first thing wrong with it.
pub fn decode(bytes: &[u8]) -> Result<Gif, String> {
    let mut r = Reader { bytes, at: 0 };
    r.expect(b"GIF89a", "signature")?;
    let size = (r.u16()?, r.u16()?);
    let packed = r.u8()?;
    if packed & 0x80 == 0 {
        return Err("no global colour table".into());
    }
    let _background = r.u8()?;
    let _aspect = r.u8()?;
    let palette: Vec<[u8; 3]> = r
        .take(3 << ((packed & 7) + 1))?
        .chunks_exact(3)
        .map(|c| [c[0], c[1], c[2]])
        .collect();
    let (mut loops, mut frames, mut sub_blocks) = (None, Vec::new(), Vec::new());
    let mut delay = None;
    loop {
        match r.u8()? {
            0x21 => match r.u8()? {
                0xF9 => {
                    r.expect(&[4], "control block size")?;
                    let flags = r.u8()?;
                    if flags & 1 != 0 {
                        return Err("a transparent colour".into());
                    }
                    delay = Some(r.u16()?);
                    let _transparent = r.u8()?;
                    r.expect(&[0], "control block end")?;
                }
                0xFF => {
                    r.expect(&[11], "application block size")?;
                    r.expect(b"NETSCAPE2.0", "application")?;
                    r.expect(&[3, 1], "loop sub-block")?;
                    loops = Some(r.u16()?);
                    r.expect(&[0], "application block end")?;
                }
                label => return Err(format!("unknown extension {label:#x}")),
            },
            0x2C => {
                let place = (r.u16()?, r.u16()?, r.u16()?, r.u16()?);
                if place != (0, 0, size.0, size.1) {
                    return Err(format!("not a whole frame: {place:?}"));
                }
                if r.u8()? != 0 {
                    return Err("a local table or interlace".into());
                }
                let delay_cs = delay.take().ok_or("an image with no control block")?;
                let code_size = r.u8()?;
                if !(2..=8).contains(&code_size) {
                    return Err(format!("LZW minimum code size {code_size}"));
                }
                let (mut lzw, mut blocks) = (Vec::new(), Vec::new());
                loop {
                    let n = r.u8()? as usize;
                    if n == 0 {
                        break;
                    }
                    lzw.extend_from_slice(r.take(n)?);
                    blocks.push(n);
                }
                let indices = Decoder::new(BitOrder::Lsb, code_size)
                    .decode(&lzw)
                    .map_err(|e| format!("LZW: {e}"))?;
                let pixels = size.0 as usize * size.1 as usize;
                if indices.len() != pixels {
                    return Err(format!("{} pixels, not {pixels}", indices.len()));
                }
                let rgb = indices
                    .iter()
                    .map(|&i| {
                        palette
                            .get(i as usize)
                            .copied()
                            .ok_or_else(|| format!("index {i} past the table"))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                frames.push(Frame { delay_cs, indices, rgb });
                sub_blocks.push(blocks);
            }
            0x3B => break,
            b => return Err(format!("unknown block {b:#x} at byte {}", r.at - 1)),
        }
    }
    if r.at != bytes.len() {
        return Err(format!("{} bytes after the trailer", bytes.len() - r.at));
    }
    Ok(Gif { size, palette, loops, frames, sub_blocks })
}
