//! Workflow Watch: a passive observer of how the artist works. It reads
//! events and answers with fires and notices. No UI, no I/O, no screen text,
//! and it never changes the document.
//!
//! Two clocks. `now` is real seconds since launch, unclamped: the undo loop's
//! window and the polish gap are real time, and only this session uses them.
//! Active time is the sum of frame gaps, each clamped, because frames come on
//! demand and one gap can be hours.

mod book;
mod event;
mod level;
mod spans;

pub use book::{Book, CheckRecord, CheckState, CutRecord};
pub use event::{
    CheckId, Delivery, DrawKey, Event, Evidence, Fire, FrameRange, Notice, Output, PaperBox, Spot,
    StepBox, TileBox, step_box,
};
pub use level::{
    Level, MAX_FRAME_SECS, POLISH_GAP_SECS, POLISH_REGION, POLISH_ZOOM, Thresholds, UNDO_OVERLAP,
    UNDO_WINDOW_SECS,
};
pub use spans::{KeySpan, SpanEnd, key_spans};

use crate::command::CutRef;
use crate::ids::DrawingId;

struct UndoRun {
    at: CutRef,
    key: DrawKey,
    steps: Vec<(f64, TileBox)>,
}

struct PolishRun {
    spot: Spot,
    region: PaperBox,
    worked: f64,
    last_stroke: f64,
}

struct Focus {
    at: CutRef,
    drawing: Option<DrawingId>,
    /// Active time before the first stroke gave the drawing an id.
    held: f64,
}

/// The detectors and the book they write to.
pub struct Watch {
    pub book: Book,
    project: String,
    last_frame: Option<f64>,
    zoom: f32,
    in_range: bool,
    focus: Option<Focus>,
    undo: Option<UndoRun>,
    polish: Option<PolishRun>,
}

impl Watch {
    pub fn new(book: Book) -> Self {
        Self {
            book,
            project: String::new(),
            last_frame: None,
            zoom: 1.0,
            in_range: false,
            focus: None,
            undo: None,
            polish: None,
        }
    }

    /// A document swap. Ids restart in every project, so everything held in
    /// memory goes, and so do the counters of a document that was never saved.
    pub fn open_project(&mut self, path: &str) {
        self.book.drop_unsaved();
        self.project = path.to_string();
        self.focus = None;
        self.undo = None;
        self.polish = None;
    }

    /// The open project was saved under `path`. A first save carries its
    /// counters over; a Save As of a named project starts them afresh.
    pub fn project_saved(&mut self, path: &str) {
        if self.project.is_empty() {
            self.book.adopt_unsaved(path);
        }
        self.project = path.to_string();
    }

    /// Feed one event at `now`, real seconds since launch.
    pub fn feed(&mut self, now: f64, event: Event) -> Vec<Output> {
        let mut out = Vec::new();
        if !self.book.enabled {
            self.last_frame = None;
            return out;
        }
        if self.last_frame.is_none() {
            self.last_frame = Some(now);
        }
        match event {
            Event::Frame { pen_down } => {
                let dt = self.last_frame.map_or(0.0, |t| (now - t).clamp(0.0, MAX_FRAME_SECS));
                self.last_frame = Some(now);
                self.clock_active(dt, pen_down);
                self.tick_polish(now, dt, pen_down, &mut out);
            }
            Event::Stroke { spot, area, erased, paper_area } => {
                self.assign_focus(spot);
                self.stroke_polish(now, spot, area, paper_area);
                if erased {
                    self.undo_step(now, spot, TileBox::of_paper(area), &mut out);
                }
            }
            Event::UndoOrErase { spot, tiles } => self.undo_step(now, spot, tiles, &mut out),
            Event::Focus { at, drawing } => self.change_focus(at, drawing),
            Event::Zoom { zoom } => {
                self.zoom = zoom;
                if zoom < POLISH_ZOOM {
                    self.polish = None;
                }
            }
            Event::Proximity { in_range } => self.in_range = in_range,
            Event::Spans { at, spans } => self.judge_spans(at, &spans, &mut out),
            Event::CutFinished { at } => {
                let notices = self.book.finish_cut(&self.project, at.cut);
                out.extend(notices.into_iter().map(Output::Notice));
            }
        }
        out
    }

    fn fire(
        &mut self,
        check: CheckId,
        at: CutRef,
        drawing: Option<DrawingId>,
        frames: FrameRange,
        evidence: Evidence,
        out: &mut Vec<Output>,
    ) {
        let Some(delivery) = self.book.delivery(check) else { return };
        self.book.add_fire(&self.project, at.cut, check);
        out.push(Output::Fire(Fire { check, at, drawing, frames, evidence, delivery }));
    }

    // ---- active time, and the key check -------------------------------

    fn clock_active(&mut self, dt: f64, pen_down: bool) {
        if !(pen_down || self.in_range) || self.book.delivery(CheckId::KeyTime).is_none() {
            return;
        }
        let Some(focus) = self.focus.as_mut() else { return };
        match focus.drawing {
            Some(id) => self.book.add_active(&self.project, focus.at.cut, id, dt),
            None => focus.held += dt,
        }
    }

    /// The first stroke on an empty cell gives the focus its drawing.
    fn assign_focus(&mut self, spot: Spot) {
        let (Some(focus), Some(id)) = (self.focus.as_mut(), spot.drawing) else { return };
        if focus.at == spot.at && focus.drawing.is_none() {
            focus.drawing = Some(id);
            let held = std::mem::take(&mut focus.held);
            self.book.add_active(&self.project, spot.at.cut, id, held);
        }
    }

    fn change_focus(&mut self, at: CutRef, drawing: Option<DrawingId>) {
        let same_cut = self.focus.as_ref().is_some_and(|f| f.at == at);
        if same_cut && self.focus.as_ref().is_some_and(|f| f.drawing == drawing) {
            return;
        }
        self.focus = Some(Focus { at, drawing, held: 0.0 });
        if !same_cut {
            self.undo = None;
            self.polish = None;
        } else if self.polish.as_ref().is_some_and(|p| p.spot.drawing != drawing) {
            self.polish = None;
        }
    }

    fn judge_spans(&mut self, at: CutRef, spans: &[KeySpan], out: &mut Vec<Output>) {
        if self.book.delivery(CheckId::KeyTime).is_none() {
            return;
        }
        let ratio = self.book.level.thresholds().key_ratio;
        for span in spans {
            let times: Vec<f64> = span
                .inbetweens
                .iter()
                .map(|id| self.book.active(&self.project, at.cut, *id))
                .collect();
            // Duplicates and pasted art have no time, and prove nothing.
            if times.is_empty() || times.iter().any(|t| *t <= 0.0) {
                continue;
            }
            let mean = times.iter().sum::<f64>() / times.len() as f64;
            for end in [span.from, span.to] {
                let key_time = self.book.active(&self.project, at.cut, end.drawing);
                if !end.is_key
                    || key_time <= 0.0
                    || key_time >= ratio * mean
                    || !self.book.tell_key(&self.project, at.cut, end.drawing)
                {
                    continue;
                }
                let mut ids = vec![end.drawing];
                ids.extend(&span.inbetweens);
                let drawing = Some(end.drawing);
                let evidence = Evidence::Drawings(ids);
                self.fire(CheckId::KeyTime, at, drawing, span.frames, evidence, out);
            }
        }
    }

    // ---- the undo loop ------------------------------------------------

    fn undo_step(&mut self, now: f64, spot: Spot, tiles: TileBox, out: &mut Vec<Output>) {
        if self.book.delivery(CheckId::UndoLoop).is_none() {
            return;
        }
        let run = self.undo.take().and_then(|mut run| {
            run.steps.retain(|(t, _)| now - *t <= UNDO_WINDOW_SECS);
            let joins = run.at == spot.at
                && run.key == spot.key
                && run.steps.last().is_some_and(|(_, b)| b.overlap(tiles) >= UNDO_OVERLAP);
            joins.then_some(run)
        });
        let mut run = run.unwrap_or(UndoRun { at: spot.at, key: spot.key, steps: Vec::new() });
        run.steps.push((now, tiles));
        if run.steps.len() < self.book.level.thresholds().undo_count {
            self.undo = Some(run);
            return;
        }
        let area = run.steps.iter().map(|(_, b)| *b).reduce(TileBox::union).unwrap_or(tiles);
        let evidence = Evidence::Area(area.paper());
        self.fire(CheckId::UndoLoop, spot.at, spot.drawing, spot.frames, evidence, out);
    }

    // ---- over-polishing -----------------------------------------------

    fn stroke_polish(&mut self, now: f64, spot: Spot, area: PaperBox, paper_area: f64) {
        if self.zoom < POLISH_ZOOM || self.book.delivery(CheckId::OverPolish).is_none() {
            self.polish = None;
            return;
        }
        let limit = paper_area * POLISH_REGION;
        if let Some(run) = self.polish.as_mut() {
            let region = run.region.union(area);
            let same = run.spot.at == spot.at && run.spot.key == spot.key;
            if same && now - run.last_stroke <= POLISH_GAP_SECS && region.area() < limit {
                run.region = region;
                run.last_stroke = now;
                run.spot = spot;
                return;
            }
        }
        // Work moved: the timer starts again on this stroke alone.
        self.polish = (area.area() < limit).then_some(PolishRun {
            spot,
            region: area,
            worked: 0.0,
            last_stroke: now,
        });
    }

    fn tick_polish(&mut self, now: f64, dt: f64, pen_down: bool, out: &mut Vec<Output>) {
        let Some(run) = self.polish.as_mut() else { return };
        if now - run.last_stroke > POLISH_GAP_SECS {
            self.polish = None;
            return;
        }
        if pen_down || self.in_range {
            run.worked += dt;
        }
        if run.worked < self.book.level.thresholds().polish_secs {
            return;
        }
        let Some(run) = self.polish.take() else { return };
        let (spot, evidence) = (run.spot, Evidence::Area(run.region));
        self.fire(CheckId::OverPolish, spot.at, spot.drawing, spot.frames, evidence, out);
    }
}
