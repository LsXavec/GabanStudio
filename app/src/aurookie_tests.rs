//! AU23: the three Rookie lessons: what their files hold and what their art
//! promises (`research/LESSONS-approachable-ui.md`). The one gate walks them with
//! every other lesson; this reads what only these three say.

use std::collections::BTreeSet;
use std::path::PathBuf;

use anim_core::manga;
use eframe::egui::pos2;

use crate::auguide_show_tests::{
    File, Hand, Picture, Ridden, at, bounds_a_region, contact_sheet, flood, iou, ride, settle, thumb, trace_load,
};
use crate::aulesson_tests::open;
use crate::aurail_tests::sized;
use crate::canvas::CanvasTool;
use crate::config::Action;
use crate::lesson::SHOW_ME;
use crate::lessons::art::{self, Pt};
use crate::lessons::{
    self, BODY_SEED, Class, DESK_EDGE, FEET, Lesson, P2_PAPER, PENCIL_SEED, Path, PathKind, SHADOW_SEED, SKY_SEED, STARS,
    STRIP_LONG, Show, desk_texel, pose_spine,
};
use crate::plate;
use crate::render_queue::ExportKind;
use crate::steps::ProjectKind;
use crate::testkit::EditorHarness;
use crate::workspace::Stage;
use crate::templates;

/// How far a pixel may stand from the colour a lesson promises.
const NEAR: i32 = 8;
/// `Picture`'s eight bits against the guide's blue: the spec's margin.
const GUIDE_NEAR: i32 = 3;
const PANELS: [[i32; 4]; 4] = [[90, 102, 440, 610], [449, 102, 798, 610], [90, 634, 440, 1141], [449, 634, 798, 1141]];

fn rookie(id: &str) -> Lesson {
    *lessons::ALL.iter().find(|l| l.id == id).expect("a Rookie lesson")
}

fn near(a: [u8; 4], b: [u8; 3], by: i32) -> bool {
    (0..3).all(|i| (i32::from(a[i]) - i32::from(b[i])).abs() <= by)
}

fn bright(p: [u8; 4]) -> i32 {
    p[..3].iter().map(|c| i32::from(*c)).sum()
}

fn guide_pixels(picture: &Picture) -> usize {
    let [r, g, b, _] = plate::GUIDE.to_array();
    picture.rgba.chunks_exact(4).filter(|p| near([p[0], p[1], p[2], p[3]], [r, g, b], GUIDE_NEAR)).count()
}

/// `id`, by Show me, on a first run at a laptop's size.
fn ridden(id: &str, hand: Hand) -> (EditorHarness, Ridden) {
    let lesson = rookie(id);
    let mut h = EditorHarness::as_first_run();
    sized(&mut h, 1280.0, 800.0);
    open(&mut h, id);
    let file = ride(&mut h, &lesson, hand);
    (h, file)
}

fn landed(ride: &Ridden) -> &File {
    assert_eq!(ride.files.len(), 1, "one file");
    &ride.files[0]
}

/// Lesson text as the lessons file gives it, but for the two sentences whose
/// `cut` the word rule refuses (`aulesson_no_old_words`).
fn in_the_file(sentence: &str) -> String {
    sentence.replace("is split into boxes", "is cut into boxes").replace("You can split a page", "You can cut a page")
}

/// Spec AU23: three lessons of the first class, one of each type, on papers no
/// picker offers, each sentence the lessons file's, and Start's second row.
#[test]
fn aurookie_three_stand() {
    let ids: Vec<&str> = lessons::ALL.iter().map(|l| l.id).collect();
    assert_eq!(ids, ["up_late", "end_of_the_desk", "not_a_pencil"]);
    assert!(lessons::ALL.iter().all(|l| l.class == Class::Rookie));
    let kinds: Vec<ProjectKind> = lessons::ALL.iter().map(|l| l.kind).collect();
    assert_eq!(kinds, [ProjectKind::Drawing, ProjectKind::Animation, ProjectKind::Comic]);
    let sizes: Vec<(u32, u32)> = lessons::ALL.iter().map(|l| (l.paper.width, l.paper.height)).collect();
    assert_eq!(sizes, [(1200, 1200), (1024, 768), (888, 1243)]);
    for lesson in lessons::ALL {
        let name = lesson.paper.name;
        assert!(!templates::ALL.iter().chain(templates::INTENTS).any(|t| t.name == name), "{name} is a lesson's alone");
    }
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../research/LESSONS-approachable-ui.md");
    let file = std::fs::read_to_string(file).expect("the lessons file").replace("\\|", "|");
    let mut read = 0;
    for lesson in lessons::ALL {
        let mut sentences = vec![lesson.closing];
        for task in lesson.tasks {
            sentences.extend(std::iter::once(task.say).chain(task.why).chain(task.miss));
        }
        for sentence in sentences {
            assert!(file.contains(&in_the_file(sentence)), "{}: {sentence:?} is not the lessons file's", lesson.id);
            read += 1;
        }
    }
    assert!(read > 58, "{read} sentences");
    assert_ne!(in_the_file("is split into boxes"), "is split into boxes", "the check reads");

    // Row two is the lessons' now, and the example stands on their page.
    let mut h = EditorHarness::as_first_run();
    settle(&mut h);
    assert!(h.has("Follow a lesson"), "{}", h.describe());
    assert!(!h.has("Open the example"), "the example left Start's row");
    h.click("Follow a lesson");
    settle(&mut h);
    assert!(h.has("Open the example"), "{}", h.describe());
    lessons::ALL.iter().for_each(|l| assert!(h.has(l.title), "{}", l.title));
}

/// Spec AU23 acceptance 3.
#[test]
fn aurookie_up_late_file() {
    let (_h, ride) = ridden("up_late", Hand::ShowMe);
    let File::One(ExportKind::Still, path) = landed(&ride) else { panic!("a PNG") };
    let picture = Picture::png(path);
    assert_eq!(picture.size, (1200, 1200));
    let seed = |s: [f32; 2]| picture.pixel(s[0] as i32, s[1] as i32);
    let [nr, ng, nb, _] = plate::LESSON_NIGHT.to_array();
    let sky = seed(SKY_SEED);
    assert!(near(sky, [nr, ng, nb], NEAR), "night at the sky seed: {sky:?}");
    let body = seed(BODY_SEED);
    assert!(near(body, [255, 255, 255], NEAR), "white at the body seed: {body:?}");
    let pencil = seed(PENCIL_SEED);
    assert!(pencil[0] > 170 && pencil[1] < 110 && pencil[2] < 110, "red at the pencil seed: {pencil:?}");
    for star in STARS {
        // Glow burns the middle pale; it is still more yellow than white.
        let centre = seed(star);
        assert!(centre[0] > 240 && centre[1] > 240 && centre[2] < centre[1], "a yellow star at {star:?}: {centre:?}");
        let halo = picture.pixel(star[0] as i32 + 30, star[1] as i32);
        assert!(bright(halo) > bright(sky), "Glow's halo at {star:?}: {halo:?} on {sky:?}");
    }
    // The smudge's box: nothing but night, and the eraser's wipe past it.
    for y in 992..1066 {
        for x in 176..286 {
            let p = picture.pixel(x, y);
            assert!(near(p, [nr, ng, nb], NEAR), "a smudge left at ({x}, {y}): {p:?}");
        }
    }
    assert_eq!(guide_pixels(&picture), 0, "no guide colour");
}

/// Spec AU23 acceptance 6: the lines leak, the sentence says so, and Undo and
/// Show me recover it.
#[test]
fn aurookie_night_leak_recovers() {
    const BACK_ONLY: &[u16] = &[0];
    let lesson = rookie("up_late");
    let night = lesson.tasks.iter().position(|t| t.miss.is_some()).expect("the leak's task");
    let mut h = EditorHarness::as_first_run();
    sized(&mut h, 1280.0, 800.0);
    open(&mut h, lesson.id);
    h.editor_mut().enter_room(Stage::InkPaint);
    h.editor_mut().begin_lesson(lesson, night);
    settle(&mut h);
    assert!(h.editor_mut().lesson_land(BACK_ONLY), "one side of Deckle, the other not drawn");
    settle(&mut h);
    let e = h.editor_mut();
    e.canvas.brush_color = plate::LESSON_NIGHT.to_array();
    e.canvas.set_tool(CanvasTool::Fill, &mut e.state);
    e.canvas.fill_at(pos2(SKY_SEED[0], SKY_SEED[1]), &mut e.state);
    settle(&mut h);
    assert_eq!(at(&h), night, "a leak ticks nothing");
    let words = h.painted_text().join(" ");
    assert!(words.contains("Night got into Deckle."), "{words}");
    h.perform(Action::Undo);
    settle(&mut h);
    assert_eq!(at(&h), night, "still here");
    h.click(SHOW_ME);
    settle(&mut h);
    assert_eq!(at(&h), night + 1, "Show me closed the gap and poured");
    assert!(!h.painted_text().join(" ").contains("Night got into Deckle."));
}

/// `Press the blue dot`, by a mouse: its bare click lands the dot, and away from
/// the dot it stays what it is on any paper, nothing.
#[test]
fn aurookie_a_click_presses_the_dot() {
    let lesson = rookie("end_of_the_desk");
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1280.0, 800.0);
    open(&mut h, lesson.id);
    settle(&mut h);
    let [x, y, _] = (lesson.art)()[1].points[0];
    let xf = h.editor().canvas.view_xform();
    let before = h.state().engine.revision();
    h.click_pos(xf.paper_to_screen(pos2(x + 120.0, y)));
    settle(&mut h);
    assert_eq!(h.state().engine.revision(), before, "off the dot");
    h.click_pos(xf.paper_to_screen(pos2(x, y)));
    settle(&mut h);
    assert_ne!(h.state().engine.revision(), before, "on the dot");
}

/// Spec AU23 acceptance 4.
#[test]
fn aurookie_end_of_the_desk_file() {
    const CHALK: i32 = 200 * 3;
    // Glow lights the torn stub in the first peg too, and the stub stays.
    const PEG_BAR_ENDS: i32 = 240;
    let (h, ride) = ridden("end_of_the_desk", Hand::ShowMe);
    let File::One(ExportKind::Gif, path) = landed(&ride) else { panic!("a GIF") };
    // The encoder holds a repeated picture rather than writing it again.
    let (pictures, length) = Picture::gif(path);
    assert_eq!(h.state().shown_length(), 15, "frames");
    assert_eq!(length, 125, "centiseconds");
    let bytes = std::fs::read(path).expect("the GIF");
    assert!(bytes.windows(19).any(|w| w == b"\x21\xFF\x0BNETSCAPE2.0\x03\x01\x00\x00\x00"), "it loops for ever");
    let mut distinct: Vec<&Picture> = Vec::new();
    for picture in &pictures {
        if distinct.last().is_none_or(|last| last.rgba != picture.rgba) {
            distinct.push(picture);
        }
    }
    assert_eq!(distinct.len(), 6, "pictures");
    let mass = |picture: &Picture| {
        let (w, h) = (picture.size.0 as i32, picture.size.1 as i32);
        let chalk: Vec<i32> = (0..h).flat_map(|y| (PEG_BAR_ENDS..w).map(move |x| (x, y)))
            .filter(|(x, y)| bright(picture.pixel(*x, *y)) >= CHALK)
            .map(|(x, _)| x)
            .collect();
        (chalk.len(), chalk.iter().sum::<i32>() as f32 / chalk.len().max(1) as f32)
    };
    let masses: Vec<(usize, f32)> = distinct[..5].iter().map(|p| mass(p)).collect();
    assert!(masses.iter().all(|m| m.0 > 500), "Deckle stands in each: {masses:?}");
    assert!(masses.windows(2).all(|m| m[1].1 >= m[0].1 - 1.0), "and never goes back: {masses:?}");
    assert!(masses[4].1 > masses[0].1 + 200.0, "his mass moves right: {masses:?}");
    // The last picture is the desk, and nobody on it.
    let last = distinct[5];
    assert_eq!(mass(last).0, 0, "nobody on the desk");
    let (w, h) = (last.size.0 as i32, last.size.1 as i32);
    for (x, y) in (0..h).step_by(13).flat_map(|y| (PEG_BAR_ENDS..w).step_by(17).map(move |x| (x, y))) {
        let [r, g, b, _] = desk_texel(x, y);
        assert!(near(last.pixel(x, y), [r, g, b], 3 * NEAR), "({x}, {y}): {:?} is no desk", last.pixel(x, y));
    }
    assert!(pictures.iter().all(|p| guide_pixels(p) == 0), "no guide colour");
}

/// Black pixels `R` in from their edge, as whole regions.
fn black_regions(picture: &Picture) -> Vec<BTreeSet<(i32, i32)>> {
    const R: i32 = 5;
    // Past an eye's press, which is a dot and not a region.
    const REGION: usize = 2000;
    let (w, h) = (picture.size.0 as i32, picture.size.1 as i32);
    let black = |x: i32, y: i32| (0..w).contains(&x) && (0..h).contains(&y) && bright(picture.pixel(x, y)) < 90;
    let solid = |x: i32, y: i32| [-R, 0, R].iter().all(|dy| [-R, 0, R].iter().all(|dx| black(x + dx, y + dy)));
    let mut left: BTreeSet<(i32, i32)> = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).filter(|(x, y)| solid(*x, *y)).collect();
    let mut regions = Vec::new();
    while let Some(first) = left.pop_first() {
        let (mut region, mut open) = (BTreeSet::from([first]), vec![first]);
        while let Some((x, y)) = open.pop() {
            for next in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
                if left.remove(&next) {
                    region.insert(next);
                    open.push(next);
                }
            }
        }
        if region.len() > REGION {
            regions.push(region);
        }
    }
    regions
}

/// Spec AU23 acceptance 5.
#[test]
fn aurookie_not_a_pencil_file() {
    let (h, ride) = ridden("not_a_pencil", Hand::ShowMe);
    let File::Pages(dir) = landed(&ride) else { panic!("a page") };
    let pages = Picture::folder(dir);
    assert_eq!(pages.len(), 1, "one page");
    let page = &pages[0];
    assert_eq!(page.size, (888, 1243));
    let (_, polys) = manga::panels_of(h.state().cut()).expect("a book");
    assert_eq!(polys.len(), 4, "panels");
    let regions = black_regions(page);
    assert_eq!(regions.len(), 1, "one black region: {:?}", regions.iter().map(|r| (r.len(), r.first())).collect::<Vec<_>>());
    let shadow = (SHADOW_SEED[0] as i32, SHADOW_SEED[1] as i32);
    assert!(regions[0].contains(&shadow), "it stands at the shadow's seed");
    assert_eq!(guide_pixels(page), 0, "no guide colour");
    for [l, t, r, b] in PANELS {
        let inked = (t + 8..b - 8).flat_map(|y| (l + 8..r - 8).map(move |x| (x, y))).filter(|(x, y)| bright(page.pixel(*x, *y)) < 300).count();
        assert!(inked > 150, "the panel at ({l}, {t}) holds {inked} px of ink");
    }
    // Each line stands in the box it was drawn for, on the page's real panels.
    let lesson = rookie("not_a_pencil");
    for (i, path) in (lesson.art)().iter().enumerate() {
        let b = path.bounds();
        let inside = |p: &[i32; 4]| b[0] >= p[0] as f32 && b[1] >= p[1] as f32 && b[2] <= p[2] as f32 && b[3] <= p[3] as f32;
        assert!(PANELS.iter().any(inside), "path {i} {b:?} crosses a gutter");
    }
    for poly in &polys {
        let mid = poly.iter().fold([0.0, 0.0], |a, p| [a[0] + p[0] / poly.len() as f32, a[1] + p[1] / poly.len() as f32]);
        assert!(PANELS.iter().any(|[l, t, r, b]| (*l..*r).contains(&(mid[0] as i32)) && (*t..*b).contains(&(mid[1] as i32))), "{poly:?}");
    }
    let paper = page.pixel(P2_PAPER[0] as i32, P2_PAPER[1] as i32);
    assert!(bright(paper) > 3 * 240, "panel 2's paper stays paper: {paper:?}");
}

/// Spec AU23 acceptance 2: the card's button alone, one press a task and the
/// save dialog. A task it could not answer would stop the ride by name.
#[test]
fn aurookie_show_me_alone_reaches_each_file() {
    for lesson in lessons::ALL {
        assert!(lesson.tasks.iter().all(|t| !matches!(t.show, Show::Where)), "{}: every task shows", lesson.id);
        let (_h, ride) = ridden(lesson.id, Hand::Alone);
        landed(&ride);
    }
}

/// Spec 5.14 on the three: a load of six, presses not counted.
#[test]
fn aurookie_trace_load() {
    for lesson in lessons::ALL {
        let load = trace_load(lesson);
        assert!((1..=6).contains(&load), "{}: {load}", lesson.id);
    }
}

/// The lines these paths draw, with none of the eraser's or the seeds'.
fn lines(lesson: &Lesson, paths: &[usize]) -> Vec<Path> {
    let art = (lesson.art)();
    paths.iter().map(|i| Path { points: art[*i].points.clone(), ..Path::new(Vec::new(), art[*i].width) }).collect()
}

/// Spec AU23 acceptance 7: every seed a lesson fills bounds its region, at the
/// default gap and at the lessons' own, and the sky stays out of Deckle.
#[test]
fn aurookie_shapes_close() {
    let up_late = rookie("up_late");
    let paper = (1200, 1200);
    for gap in [2, 8] {
        let deckle = lines(&up_late, &[0, 1]);
        assert!(bounds_a_region(&deckle, BODY_SEED, paper, gap), "Deckle at gap {gap}");
        let pencil = lines(&up_late, &[5]);
        assert!(bounds_a_region(&pencil, PENCIL_SEED, paper, gap), "the pencil at gap {gap}");
        let all = lines(&up_late, &[0, 1, 5]);
        let sky = flood(&all, (SKY_SEED[0] as i32, SKY_SEED[1] as i32), paper, gap).expect("the sky");
        assert!(!sky.get(BODY_SEED[0] as i32, BODY_SEED[1] as i32), "the sky stays out at gap {gap}");
        assert!(!sky.get(PENCIL_SEED[0] as i32, PENCIL_SEED[1] as i32), "and out of the pencil");
        assert!(sky.get(560, 790), "and reaches under the arch at gap {gap}");
    }
    let edge = rookie("end_of_the_desk");
    let paper = (edge.paper.width, edge.paper.height);
    for i in 0..FEET.len() {
        let pose = lines(&edge, &[2 * i]);
        assert!(bounds_a_region(&pose, lessons::pose_seed(i), paper, 8), "pose {i}");
    }
    let page = rookie("not_a_pencil");
    let paper = (page.paper.width, page.paper.height);
    let art: Vec<usize> = (0..(page.art)().len()).collect();
    assert!(bounds_a_region(&lines(&page, &art), SHADOW_SEED, paper, 8), "the shadow");
}

/// Spec AU23 acceptance 8.
#[test]
fn aurookie_kinds_share_no_paths() {
    let arts: Vec<Vec<Path>> = lessons::ALL.iter().map(|l| (l.art)()).collect();
    for (i, art) in arts.iter().enumerate() {
        for other in &arts[i + 1..] {
            for a in art {
                assert!(!other.iter().any(|b| a.points == b.points), "a path stands in two lessons");
            }
        }
    }
    // `pose_spine` and the arch are one line of work with two seeds at most.
    assert_ne!(art::deckle(&pose_spine(0), 4).cut.points, art::deckle(&pose_spine(0), 5).cut.points, "the check reads");
}

/// Spec AU23: another seed would pass the last test, so the pictures are read:
/// each lesson's whole drawing at 64×64.
#[test]
fn aurookie_kinds_look_different() {
    let drawn: Vec<(&str, Vec<bool>)> = lessons::ALL.iter().map(|l| {
        let art = (l.art)();
        let ink: Vec<&Path> = art.iter().filter(|p| matches!(p.kind, PathKind::Guide)).collect();
        (l.id, thumb(&ink, (l.paper.width, l.paper.height)))
    }).collect();
    for (i, (a, ink)) in drawn.iter().enumerate() {
        assert!(ink.iter().filter(|c| **c).count() > 100, "{a} has a drawing");
        for (b, other) in &drawn[i + 1..] {
            let shared = iou(ink, other);
            assert!(shared <= 0.45, "{a} and {b} share {shared}");
        }
    }
}

/// Spec 5.13: paper does not stretch and a foot that carries does not skate.
#[test]
fn aurookie_poses_stay_on_model() {
    const STAYS: f32 = 2.0;
    let spines: Vec<Vec<Pt>> = (0..FEET.len()).map(pose_spine).collect();
    for (i, spine) in spines.iter().enumerate() {
        let long = art::length(spine);
        assert!((long - STRIP_LONG).abs() <= STRIP_LONG * 0.01, "pose {i}: {long}");
        assert!((spine[0][0] - FEET[i][0]).abs() <= STAYS, "pose {i}'s tail: {:?}", spine[0]);
        let head = spine[spine.len() - 1];
        assert!((head[0] - FEET[i][1]).abs() <= STAYS || head[0] > DESK_EDGE, "pose {i}'s head: {head:?}");
    }
    for (i, pair) in spines.windows(2).enumerate() {
        let ends = |s: &[Pt]| [s[0], s[s.len() - 1]];
        let ([tail, head], [next_tail, next_head]) = (ends(&pair[0]), ends(&pair[1]));
        let far = |a: Pt, b: Pt| (a[0] - b[0]).hypot(a[1] - b[1]);
        assert!(far(tail, next_tail).min(far(head, next_head)) <= STAYS, "poses {i} and {}: both feet slid", i + 1);
    }
    let up_late = rookie("up_late");
    let art = (up_late.art)();
    assert!(art[0].points.iter().all(|p| p[1] >= 500.0), "the arch's peak stands where the lessons file puts it");
}

/// Spec AU23 build 5: the sheet writes, with every lesson's finish on it.
#[test]
fn aurookie_contact_sheet_writes() {
    let dir = tempfile::tempdir().expect("dir");
    let (file, cells) = contact_sheet(dir.path());
    let sheet = Picture::png(&file);
    assert_eq!(cells.len(), lessons::ALL.len(), "a cell each");
    for [l, t, r, b] in cells {
        let inked = (t..b).flat_map(|y| (l..r).map(move |x| (x, y))).filter(|(x, y)| bright(sheet.pixel(*x, *y)) < 3 * 128).count();
        assert!(inked > 200, "the cell at ({l}, {t}) has {inked} px");
    }
}
