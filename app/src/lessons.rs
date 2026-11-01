//! AU20: the lesson table, as the Lessons page reads it (spec 4.5). `ALL` stays
//! empty until a lesson ships: while it is, Start keeps `Open the example` on
//! its second row and the page cannot be reached. A lesson's paper and tasks
//! come with the lesson frame (spec 5.3).

use crate::steps::ProjectKind;

/// Who a lesson is for: a column of the page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    Rookie,
    Amateur,
    Experienced,
}

impl Class {
    pub const ALL: [Class; 3] = [Class::Rookie, Class::Amateur, Class::Experienced];

    pub fn word(self) -> &'static str {
        match self {
            Class::Rookie => "Rookie",
            Class::Amateur => "Amateur",
            Class::Experienced => "Experienced",
        }
    }
}

/// A line of a lesson's art, in paper units (spec 5.13).
pub struct Path {
    /// `(x, y, pressure)`.
    pub points: Vec<[f32; 3]>,
    pub width: f32,
}

pub struct Lesson {
    /// Stored in `app.lesson`, never renumbered. The lesson frame reads it.
    #[allow(dead_code)]
    pub id: &'static str,
    pub kind: ProjectKind,
    pub class: Class,
    pub title: &'static str,
    /// One sentence, minutes, and what file it ends in.
    pub hover: &'static str,
    /// Everything the lesson draws with.
    pub art: fn() -> Vec<Path>,
    /// The paths of `art` that make its picture.
    pub finish: &'static [u16],
}

/// Every lesson that ships.
pub const ALL: &[Lesson] = &[];

/// A strip of paper, arched: the page's picture until a lesson brings its own.
fn stand_in_art() -> Vec<Path> {
    let arch = |x: f32| [x, 40.0 - 30.0 * (x / 100.0 * std::f32::consts::PI).sin(), 1.0];
    let points = (0..=20).map(|i| arch(i as f32 * 5.0)).collect();
    vec![Path { points, width: 2.0 }]
}

const fn stand_in(id: &'static str, kind: ProjectKind, class: Class, title: &'static str) -> Lesson {
    let hover = "One sentence on what it makes. About 20 minutes. Ends in a file.";
    Lesson { id, kind, class, title, hover, art: stand_in_art, finish: &[0] }
}

/// The nine rows the page is built for, with the titles the lessons will have
/// (`research/LESSONS-approachable-ui.md`): what its audits and its picture
/// stand on until `ALL` holds them. No lesson opens from here.
pub const STAND_IN: &[Lesson] = &[
    stand_in("stand-in-1", ProjectKind::Drawing, Class::Rookie, "Up Late"),
    stand_in("stand-in-2", ProjectKind::Drawing, Class::Amateur, "A Coat of Shavings"),
    stand_in("stand-in-3", ProjectKind::Drawing, Class::Experienced, "Plate One"),
    stand_in("stand-in-4", ProjectKind::Animation, Class::Rookie, "End of the Desk"),
    stand_in("stand-in-5", ProjectKind::Animation, Class::Amateur, "Still in the Coat"),
    stand_in("stand-in-6", ProjectKind::Animation, Class::Experienced, "Desk to Lamp"),
    stand_in("stand-in-7", ProjectKind::Comic, Class::Rookie, "Not a Pencil"),
    stand_in("stand-in-8", ProjectKind::Comic, Class::Amateur, "Too Much Coat"),
    stand_in("stand-in-9", ProjectKind::Comic, Class::Experienced, "The Only Word"),
];
