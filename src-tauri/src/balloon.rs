//! The balloon a line of dialogue sits in, read off the page's own pixels.
//!
//! Gemini's boxes only say roughly where a balloon is. The rule here is
//! geometric and the same for every page: the paper around the text is
//! flood-filled up to the printed outline, its holes (the letters) are filled
//! in, and that solid region is the balloon's usable interior. The white
//! cover is that interior pulled in from the outline by an inset that grows
//! with the interior's size, so it hides every letter, edge letters and last
//! lines included, and never reaches the outline, the art outside it or a
//! neighbouring balloon. The Korean is set in the largest rectangle the cover
//! holds, at a size taken from the original lettering.

/// Paper: a pixel at least this light is balloon background, darker is ink
/// or outline.
const LIGHT: u8 = 200;
/// A stroke of the lettering: darker than this.
const INK: u8 = 128;
/// How far past its lettering, in letter heights, a balloon's cover may
/// reach (plus a quarter of the text block's size).
pub const REACH: f64 = 3.0;
/// How far the inset sits from the outline, as a share of the square root of
/// the interior's area (its typical width), and its bounds in pixels.
pub const INSET: f64 = 0.025;
const INSET_MIN: i32 = 2;
const INSET_MAX: i32 = 12;
/// The space kept between the Korean and the cover's edge, as a share of the
/// text rectangle's smaller side.
pub const TEXT_PAD: f64 = 0.06;
/// How much dark art away from the lettering a balloon's cover may take in, in
/// square letters: a speck, not a face. (Balloons measured on the three
/// chapters hold none; lettering on a face held 600 px at 23 px letters.)
const STRAY: f64 = 0.5;
/// The least share of the lettering's width, and of its area, a balloon's
/// text area keeps.
const ROOM: f64 = 0.5;
/// How far, in letters, a balloon's cover may fall short of its lettering
/// on a side (an outline drawn through the edge of a letter).
const SHORT: f64 = 0.5;
/// The most, in letters, a mark running past the lettering's margin may
/// measure across its narrower side and still be lettering.
const LINE_THICK: f64 = 1.5;
/// Half the width, in letters, of the disk a balloon's cover is rounded by:
/// a leak or sliver narrower than it is not the balloon.
const ROUND: f64 = 0.75;
/// Rows of the cover's outline may be merged into one step while the step
/// gives up at most this many pixels of any row.
const STEP_SLACK: i32 = 2;

/// A balloon found around one text box: picture pixels, `[x0, y0, x1, y1)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    /// The cover's outline, clockwise from its top left: a staircase of rows.
    pub outline: Vec<[i64; 2]>,
    /// The outline's bounds.
    pub bounds: [i64; 4],
    /// Where the Korean goes.
    pub text: [i64; 4],
    /// The original lettering's letter height (horizontal text) or width
    /// (vertical text), if it could be measured.
    pub glyph: Option<f64>,
}

/// A grayscale page.
pub struct Page<'a> {
    pub w: u32,
    pub h: u32,
    pub px: &'a [u8],
}

impl Page<'_> {
    fn at(&self, x: i64, y: i64) -> u8 {
        self.px[y as usize * self.w as usize + x as usize]
    }
}

/// The balloon around the lettering at `text` (`[x0, y0, x1, y1]`, picture
/// pixels, letters `glyph` tall), with `others` the lettering of the page's
/// other balloons. Its interior is the paper around the text up to the
/// printed outline, and within an ellipse round the text reaching
/// [`REACH`] letters (plus a quarter of the text's size) past each side of it: where a balloon opens onto the white
/// of the panel or the page margin, the cover stops in that white instead of
/// running into it. None when there is no paper around the text (text over
/// art, a dark box).
pub fn find(page: &Page, text: [f64; 4], glyph: f64, others: &[[f64; 4]]) -> Option<Found> {
    let (tw, th) = (text[2] - text[0], text[3] - text[1]);
    if tw < 2.0 || th < 2.0 {
        return None;
    }
    let size = tw.max(th);
    let reach = REACH * glyph + 0.25 * size;
    // The reach is an ellipse round the text, as a balloon is: cut at a
    // distance from the text's rectangle, paper running out through a gap in
    // the outline ended in straight lines across the art (ch256 p9, "DU"
    // beside "OOOH!"; ch258 p4, the ball beside "NUREMI SUIMU"). It runs
    // through the corners of the reach round the rectangle, so it holds all
    // that held: a balloon beside the text whose outline that took in still
    // keeps the text's paper from being one (ch256 p6, bricks beside
    // "RIIIN.").
    let (ox, oy) = ((text[0] + text[2]) / 2.0, (text[1] + text[3]) / 2.0);
    let (ea, eb) = (tw / 2.0 * std::f64::consts::SQRT_2 + reach, th / 2.0 * std::f64::consts::SQRT_2 + reach);
    let beyond = |x: i64, y: i64| ((x as f64 - ox) / ea).powi(2) + ((y as f64 - oy) / eb).powi(2) > 1.0;
    // The window holds the ellipse, so its edge cuts nothing straight.
    let win = [
        (ox - ea - 4.0).max(0.0).floor() as i64,
        (oy - eb - 4.0).max(0.0).floor() as i64,
        (ox + ea + 4.0).min(page.w as f64).ceil() as i64,
        (oy + eb + 4.0).min(page.h as f64).ceil() as i64,
    ];
    let grid = Grid::new(win);
    if grid.w < 3 || grid.h < 3 {
        return None;
    }

    // The paper around the text: the light region holding most of the light
    // pixels in the middle of the text box.
    let light: Vec<bool> = grid.cells().map(|(x, y)| page.at(x, y) >= LIGHT).collect();
    let core = |b: [f64; 4]| {
        let (mx, my) = ((b[2] - b[0]) * 0.25, (b[3] - b[1]) * 0.25);
        [b[0] + mx, b[1] + my, b[2] - mx, b[3] - my]
    };
    let label = grid.components(&light);
    let vote = |r: [f64; 4]| {
        let mut votes = std::collections::HashMap::new();
        for i in grid.inside(r) {
            if light[i] {
                *votes.entry(label[i]).or_insert(0u32) += 1;
            }
        }
        votes.into_iter().max_by_key(|&(l, n)| (n, std::cmp::Reverse(l))).map(|(l, _)| l)
    };
    // Lettering dense enough to leave no paper in the middle of its box: the
    // paper just around it.
    let ring = 0.25 * th.min(tw);
    let paper = vote(core(text)).or_else(|| vote([text[0] - ring, text[1] - ring, text[2] + ring, text[3] + ring]))?;
    let mut comp: Vec<bool> = label.iter().map(|&l| l == paper).collect();
    // Whether the paper runs on past the reach: no outline closes it.
    let mut open_paper = false;
    for (i, (x, y)) in grid.cells().enumerate() {
        if comp[i] && beyond(x, y) {
            comp[i] = false;
            open_paper = true;
        }
    }

    // A balloon shared with another text (two lobes, an open balloon next to
    // another, or one balloon Gemini gave as two): each text keeps the part
    // nearer to it.
    let sharing: Vec<[f64; 4]> = others.iter().copied().filter(|&o| grid.inside(core(o)).any(|i| comp[i])).collect();
    if !sharing.is_empty() {
        for (i, (x, y)) in grid.cells().enumerate() {
            if comp[i] {
                let own = rect_dist2(text, x, y);
                comp[i] = sharing.iter().all(|&o| own <= rect_dist2(o, x, y));
            }
        }
    }

    // The interior: the paper and everything it encloses. Its ink is what
    // is dark in there: the letters (paper cut off between dense lines is
    // interior, not ink).
    // Paper given up to a neighbour or past the reach is outside this
    // balloon, not enclosed by it, and so is what only it reaches.
    let given: Vec<bool> = (0..comp.len()).map(|i| label[i] == paper && !comp[i]).collect();
    let outside = grid.reach_from(&comp, &given);
    let solid: Vec<bool> = (0..comp.len()).map(|i| comp[i] || !outside[i]).collect();
    // Only the lettering's own ink must be hidden: a speck in the white of
    // the page margin the paper runs into is not.
    let g = 0.5 * glyph;
    let letters = [text[0] - g, text[1] - g, text[2] + g, text[3] + g];
    let dark: Vec<bool> = grid.cells().enumerate().map(|(i, (x, y))| solid[i] && !comp[i] && page.at(x, y) < LIGHT).collect();
    // Only marks the text holds, or no thicker than a line of it: one
    // running out past the margin that is thick is not lettering, but a
    // sound effect drawn over the balloon (ch256 p9, the "D" of "DU" beside
    // "WITH A CRAZY", which the cover took in whole).
    let marks = grid.components(&dark);
    let mut extent: std::collections::HashMap<u32, [i64; 4]> = std::collections::HashMap::new();
    for (i, (x, y)) in grid.cells().enumerate().filter(|&(i, _)| dark[i]) {
        let e = extent.entry(marks[i]).or_insert([x, y, x, y]);
        *e = [e[0].min(x), e[1].min(y), e[2].max(x), e[3].max(y)];
    }
    let held = |l: &u32| {
        extent.get(l).is_some_and(|e| {
            let inside = e[0] as f64 >= letters[0] && e[1] as f64 >= letters[1] && e[2] as f64 <= letters[2] && e[3] as f64 <= letters[3];
            inside || ((e[2] - e[0]).min(e[3] - e[1]) as f64) <= LINE_THICK * glyph
        })
    };
    let own: std::collections::HashSet<u32> = grid.inside(text).filter(|&i| dark[i]).map(|i| marks[i]).filter(held).collect();
    let lettering = |i: usize| dark[i] && own.contains(&marks[i]);
    // All of each the size of a letter, though it reach past the margin
    // (the top of an "AH..." lettered over the balloon's edge, ch258 p15).
    let letter_px = LETTER_MAX * page.w as f64;
    let small = |l: u32| extent.get(&l).is_some_and(|e| (((e[2] - e[0]).max(e[3] - e[1])) as f64) < letter_px);
    let in_letters = |i: usize| { let (x, y) = grid.xy(i); x as f64 >= letters[0] && x as f64 <= letters[2] && y as f64 >= letters[1] && y as f64 <= letters[3] };
    let ink: Vec<usize> = (0..dark.len()).filter(|&i| lettering(i) && (in_letters(i) || small(marks[i]))).collect();
    // Marks the size of a letter that the interior holds beside the text:
    // lettering too, this text's or a neighbour's ("..." beside a large
    // "?!", ch256 p8). Not a speaker's portrait in a lobe of its own
    // (ch256 p5), which rounding may leave out.
    let near = |e: &[i64; 4]| e[0] as f64 <= text[2] + glyph && e[2] as f64 >= text[0] - glyph && e[1] as f64 <= text[3] + glyph && e[3] as f64 >= text[1] - glyph;
    let enclosed: Vec<usize> = (0..dark.len()).filter(|&i| dark[i] && extent.get(&marks[i]).is_some_and(|e| near(e) && (((e[2] - e[0]).max(e[3] - e[1])) as f64) < letter_px)).collect();
    if ink.is_empty() {
        return None;
    }
    // Lettering edged in white on a tone that no outline closes is drawn
    // on the art: the "paper" is the panel's gray (or the sky) running up
    // to the next balloon's edge, and a cover over it would join the two
    // (ch257 p6, "WHAT KIND OF A FATHER..."). A toned balloon closes round
    // its white-edged lettering (ch256 p8, "EE HEE HEE..."), and in a white
    // balloon what hugs the letters is no lighter than the paper.
    if open_paper && halo(page, &grid, &comp, &ink) {
        return None;
    }

    // A balloon has paper wider than half a letter somewhere; lettering
    // drawn on the art with a white edge round each letter has only that
    // edge.
    let open = grid.distance(&comp).into_iter().max().unwrap_or(0);
    if (open as f64) < 0.5 * glyph {
        return None;
    }

    // Rounded as a balloon is: the paper, its lettering filled in, is what
    // a disk [`ROUND`] letters across reaches rolled round inside it, from
    // the text. Paper past a gap narrower than the disk is not this
    // balloon's, however wide it opens up again: the white of an outlined
    // sound effect drawn over the outline (ch256 p9, the "D" of "DU", whose
    // bar the cover took in), or a sliver between the outline and the art
    // (ch258 p4, beside the ball). The widest disk that still leaves every
    // stroke under the cover is taken, down to none.
    let lettered: Vec<bool> = (0..comp.len()).map(|i| comp[i] || lettering(i)).collect();
    let inner = grid.distance(&lettered);
    let shape = |r: i32| -> Option<(Vec<bool>, Vec<bool>, Vec<usize>, Vec<Step>)> {
        let solid = if r == 0 {
            solid.clone()
        } else {
            let core: Vec<bool> = inner.iter().map(|&d| d > r).collect();
            let label = grid.components(&core);
            let held: std::collections::HashSet<u32> = grid.inside(text).map(|i| label[i]).filter(|&l| l != 0).collect();
            let core: Vec<bool> = label.iter().map(|l| held.contains(l)).collect();
            let near = grid.distance(&core.iter().map(|c| !c).collect::<Vec<bool>>());
            let paper: Vec<bool> = (0..comp.len()).map(|i| lettered[i] && (core[i] || near[i] <= r)).collect();
            let outside = grid.reach_from(&paper, &given);
            (0..comp.len()).map(|i| paper[i] || !outside[i]).collect()
        };
        // Rounding gives up no letter: only paper and the art.
        if ink.iter().chain(&enclosed).any(|&i| !solid[i]) {
            return None;
        }
        // The inset: a share of the interior's size, but always short of
        // the nearest letter so every stroke stays under the cover. A stroke
        // pixel touching the boundary is where the lettering meets an
        // outline (a tail drawn from the text): it stays with the outline.
        let dist = grid.distance(&solid);
        let ink: Vec<usize> = ink.iter().copied().filter(|&i| dist[i] > 1).collect();
        let area = solid.iter().filter(|&&s| s).count() as f64;
        let nearest_ink = ink.iter().map(|&i| dist[i]).min()?;
        // A pixel past the nearest stroke too, where the paper has it: a
        // stroke's soft edge is lighter than ink, and left out it showed as
        // a gray sliver (ch258 p4, the "I" of "NUREMI").
        let inset = ((INSET * area.sqrt()).round() as i32).clamp(INSET_MIN, INSET_MAX).min(nearest_ink - 2).max(1);
        let cover: Vec<bool> = dist.iter().map(|&d| d > inset).collect();
        let (ix0, ix1) = ink.iter().fold((i64::MAX, i64::MIN), |(a, b), &i| (a.min(grid.xy(i).0), b.max(grid.xy(i).0)));
        let rows = grid.runs(&cover, &ink, (ix0 + ix1) / 2);
        // The rim where the steps can keep it, but no shape is given up for it.
        let ink_xy: Vec<(i64, i64)> = ink.iter().map(|&i| grid.xy(i)).collect();
        let rim: Vec<(i64, i64)> = ink.iter().flat_map(|&i| grid.neighbours(i)).filter(|&n| cover[n]).map(|i| grid.xy(i)).chain(ink_xy.iter().copied()).collect();
        let s = [(STEP_SLACK, &rim), (0, &rim), (STEP_SLACK, &ink_xy), (0, &ink_xy)].into_iter().map(|(slack, must)| (steps(&rows, slack), must)).find(|(s, must)| covers(s, must))?.0;
        Some((solid, cover, ink, s))
    };
    let r0 = (ROUND * glyph).round() as i32;
    let (picked, (interior, _, ink, steps)) = (0..=4).map(|k| r0 * (4 - k) / 4).find_map(|r| shape(r).map(|s| (r, s)))?;
    let (ix0, ix1) = ink.iter().fold((i64::MAX, i64::MIN), |(a, b), &i| (a.min(grid.xy(i).0), b.max(grid.xy(i).0)));
    // Across, the middle of the lettering's ink, not of its extent: a few
    // letters off to one side (an "WH...?!" above the man's head, ch256 p7)
    // do not draw the Korean off the bulk of it.
    let cx = (ink.iter().map(|&i| grid.xy(i).0).sum::<i64>() as f64 / ink.len().max(1) as f64).round() as i64;
    // All a balloon holds is its lettering. A cover that would also take in
    // art away from the lettering, well inside its edge (white skin round a
    // closed eye, with lettering on it), is on the picture, not in a
    // balloon. A sliver of art at the edge, where a balloon meets the panel
    // border, is not that.
    let held = |x: i64, y: i64| steps.iter().any(|s| y >= s.top && y < s.bottom && x >= s.l && x < s.r);
    // Every letter of the text in the balloon: one lettered over its edge,
    // half out on the art, is not hidden by its cover (ch258 p15,
    // "AH..." over "SORRY... I JUST..."), and the lettering is left to the
    // cover drawn round lettering on the art.
    let inked: Vec<bool> = grid.cells().map(|(x, y)| page.at(x, y) < LIGHT).collect();
    let pieces = grid.components(&inked);
    let mut tally: std::collections::HashMap<u32, ([i64; 4], usize, usize, usize)> = std::collections::HashMap::new();
    for (i, (x, y)) in grid.cells().enumerate().filter(|&(i, _)| inked[i]) {
        let t = tally.entry(pieces[i]).or_insert(([x, y, x, y], 0, 0, 0));
        t.0 = [t.0[0].min(x), t.0[1].min(y), t.0[2].max(x), t.0[3].max(y)];
        t.1 += 1;
        t.2 += usize::from(!interior[i]);
        t.3 += usize::from(held(x, y));
    }
    let of_text: std::collections::HashSet<u32> = grid.inside(text).filter(|&i| inked[i]).map(|i| pieces[i]).collect();
    let mut letters_of = vec![];
    for &(e, n, out, _) in of_text.iter().filter_map(|l| tally.get(l)) {
        let inside = e[0] as f64 >= letters[0] && e[1] as f64 >= letters[1] && e[2] as f64 <= letters[2] && e[3] as f64 <= letters[3];
        let thin = ((e[2] - e[0]).min(e[3] - e[1]) as f64) <= LINE_THICK * glyph;
        let side = (e[2] - e[0]).max(e[3] - e[1]) as f64;
        let letter = side >= LETTER_MIN * page.w as f64 && side < letter_px;
        let (mx, my) = ((e[0] + e[2]) as f64 / 2.0, (e[1] + e[3]) as f64 / 2.0);
        let theirs = others.iter().any(|o| mx >= o[0] && mx <= o[2] && my >= o[1] && my <= o[3]);
        if (inside || thin) && letter && !theirs {
            letters_of.push((n, out));
        }
    }
    // Half in: art wholly beyond the outline is not the text's.
    let across = letters_of.iter().any(|&(n, out)| out * 4 > n && out * 10 < n * 9);
    // And most of the letters in the balloon: a patch of paper on the art
    // that a few of them touch is none (ch257 p7, which then took the line
    // "UM, UM...!" from its balloon).
    let out_of = letters_of.iter().filter(|&&(n, out)| out * 2 > n).count();
    // Nor may it cut the art: a mark of solid ink the size of a letter,
    // not the text's, partly under it (a burst beside "AH!" lettered on a
    // white wall, ch256 p9, whose cover took half of it). Not a strand of
    // toned hair reaching into a balloon (ch257 p21).
    let solid_ink: Vec<bool> = grid.cells().map(|(x, y)| page.at(x, y) < INK).collect();
    let strokes = grid.components(&solid_ink);
    let mut drawn: std::collections::HashMap<u32, ([i64; 4], usize, usize)> = std::collections::HashMap::new();
    for (i, (x, y)) in grid.cells().enumerate().filter(|&(i, _)| solid_ink[i]) {
        let t = drawn.entry(strokes[i]).or_insert(([x, y, x, y], 0, 0));
        t.0 = [t.0[0].min(x), t.0[1].min(y), t.0[2].max(x), t.0[3].max(y)];
        t.1 += 1;
        t.2 += usize::from(held(x, y));
    }
    let least = 2.0 * (LETTER_MIN * page.w as f64).powi(2);
    let cut = drawn.values().any(|&(e, n, under)| {
        let side = (e[2] - e[0]).max(e[3] - e[1]) as f64;
        let (mx, my) = ((e[0] + e[2]) as f64 / 2.0, (e[1] + e[3]) as f64 / 2.0);
        let lettering = mx >= letters[0] && mx <= letters[2] && my >= letters[1] && my <= letters[3];
        !lettering && n as f64 >= least && side < letter_px && under * 5 > n && under * 5 < n * 4
    });
    if across || cut || out_of * 2 > letters_of.len() {
        return None;
    }
    let d = (0.5 * glyph).round() as i64;
    let stray = steps
        .iter()
        .flat_map(|s| (s.top..s.bottom).flat_map(move |y| (s.l..s.r).map(move |x| (x, y))))
        .filter(|&(x, y)| page.at(x, y) < INK && rect_dist2(letters, x, y) > 0.0)
        .filter(|&(x, y)| held(x - d, y) && held(x + d, y) && held(x, y - d) && held(x, y + d))
        .count();
    if stray as f64 > STRAY * glyph * glyph {
        return None;
    }
    // Lettering edged in white over paper with art in it is drawn on the
    // art, though an outline closes the paper round it (the sky's clouds,
    // which the girl's head and the panel's edge close, ch257 p21): a toned
    // balloon round white-edged lettering holds none (ch256 p8).
    if halo(page, &grid, &comp, &ink) {
        let faint = steps
            .iter()
            .flat_map(|s| (s.top..s.bottom).flat_map(move |y| (s.l..s.r).map(move |x| (x, y))))
            .filter(|&(x, y)| page.at(x, y) < LIGHT && rect_dist2(letters, x, y) > 0.0)
            .filter(|&(x, y)| held(x - d, y) && held(x + d, y) && held(x, y - d) && held(x, y + d))
            .count();
        if faint as f64 > STRAY * glyph * glyph {
            return None;
        }
    }

    let outline = outline(&steps);
    let bounds = [
        steps.iter().map(|s| s.l).min()?,
        steps.first()?.top,
        steps.iter().map(|s| s.r).max()?,
        steps.last()?.bottom,
    ];
    // A balloon holds all its lettering, with paper round it. A cover
    // falling more than [`SHORT`] letters short of the lettering on a side
    // is not round it (a speck's patch of paper on the art, ch257 p7, which
    // then took the line "UM, UM...!" from its balloon). One no disk rounds
    // must keep a tenth of a letter clear of it: lettering wedged into nooks
    // of the art (the lines of a hand, ch257 p17), or spilling out of a
    // thought cloud (ch257 p8).
    let gap = [text[0] - bounds[0] as f64, text[1] - bounds[1] as f64, bounds[2] as f64 - text[2], bounds[3] as f64 - text[3]].into_iter().fold(f64::MAX, f64::min) / glyph;
    if gap < -SHORT || picked == 0 && gap < 0.1 {
        return None;
    }
    // Centred where the letterer centred the original.
    let (ink_y0, ink_y1) = ink.iter().fold((i64::MAX, i64::MIN), |(a, b), &i| (a.min(grid.xy(i).1), b.max(grid.xy(i).1)));
    let r = largest_rect(&steps, (cx, (ink_y0 + ink_y1) / 2))?;
    // A balloon has room for its lettering: a text area much narrower or
    // smaller than the lettering it replaces is a patch of white beside
    // lettering drawn on the art (a shout among speed lines), not a balloon.
    let (lw, lh) = ((ix1 - ix0 + 1) as f64, (ink_y1 - ink_y0 + 1) as f64);
    let (rw, rh) = ((r[2] - r[0]) as f64, (r[3] - r[1]) as f64);
    if rw < ROOM * lw || rw * rh < ROOM * lw * lh {
        return None;
    }
    let pad = ((TEXT_PAD * (r[2] - r[0]).min(r[3] - r[1]) as f64).round() as i64).max(1);
    let text = [r[0] + pad, r[1] + pad, (r[2] - pad).max(r[0] + pad + 1), (r[3] - pad).max(r[1] + pad + 1)];
    // The room the letterer used, where it is more and its lines no
    // narrower, and all of it is under the cover: the centred rectangle is
    // narrower than wide lines set off the middle. Nearly all is not
    // enough: a "!" lettered beside the balloon took the Korean over its
    // outline (ch256 p10, "HEE... HOO!").
    let lettered = [ix0, ink_y0, ix1 + 1, ink_y1 + 1];
    let held_in = |y: i64| steps.iter().find(|s| y >= s.top && y < s.bottom).map_or(0, |s| (s.r.min(lettered[2]) - s.l.max(lettered[0])).max(0));
    let under = (lettered[1]..lettered[3]).all(|y| held_in(y) == lettered[2] - lettered[0]);
    let area = |t: [i64; 4]| (t[2] - t[0]) * (t[3] - t[1]);
    let text = if under && area(lettered) > area(text) && lettered[2] - lettered[0] >= text[2] - text[0] { lettered } else { text };
    Some(Found { outline, bounds, text, glyph: Some(glyph) })
}

/// How much lighter than the paper, in gray levels, the pixels hugging the
/// letters are when each letter is edged in white.
const HALO_LIFT: f64 = 12.0;

/// Whether the light pixels two pixels off the letters' ink are clearly
/// lighter than the paper (`comp`) round them: a white edge drawn round
/// each letter.
fn halo(page: &Page, grid: &Grid, comp: &[bool], ink: &[usize]) -> bool {
    let median = |mut v: Vec<u8>| -> Option<f64> {
        v.sort_unstable();
        v.get(v.len() / 2).map(|&m| m as f64)
    };
    let paper: Vec<u8> = (0..comp.len()).step_by(3).filter(|&i| comp[i]).map(|i| { let (x, y) = grid.xy(i); page.at(x, y) }).collect();
    let mut hug = vec![];
    for &i in ink.iter().step_by(2) {
        let (x, y) = grid.xy(i);
        for (dx, dy) in [(-2i64, 0i64), (2, 0), (0, -2), (0, 2)] {
            let (nx, ny) = (x + dx, y + dy);
            if nx >= 0 && ny >= 0 && nx < page.w as i64 && ny < page.h as i64 && page.at(nx, ny) >= LIGHT {
                hug.push(page.at(nx, ny));
            }
        }
    }
    match (median(paper), median(hug)) {
        (Some(p), Some(h)) => h - p >= HALO_LIFT,
        _ => false,
    }
}

/// A block of lettering found on the page: letters (small dark marks on
/// paper) near enough to each other to be one balloon's text.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    /// `[x0, y0, x1, y1]`, picture pixels.
    pub bounds: [f64; 4],
    /// Marks it is made of: about one per letter.
    pub letters: usize,
    /// The median letter height (width, for vertical text).
    pub glyph: f64,
    /// Whether it sits on a balloon's paper ([`find`] finds one around it):
    /// marks drawn on the art (a hand, a face) seldom do.
    pub in_balloon: bool,
}

/// A letter: a dark mark between these shares of the page's width tall,
/// mostly surrounded by paper.
const LETTER_MIN: f64 = 0.006;
pub const LETTER_MAX: f64 = 0.07;
const PAPER_AROUND: f64 = 0.6;

/// Every block of lettering on the page. Gemini only says roughly where a
/// balloon is (its boxes can be off by a whole balloon), so the shapes are
/// placed on the lettering the page really shows.
pub fn blocks(page: &Page, vertical: bool) -> Vec<Block> {
    let (w, h) = (page.w as usize, page.h as usize);
    let (lo, hi) = (LETTER_MIN * page.w as f64, LETTER_MAX * page.w as f64);
    let dark: Vec<bool> = page.px.iter().map(|&v| v < INK).collect();
    let light = |x: usize, y: usize| page.px[y * w + x] >= LIGHT;
    // Dark marks, 8-connected: bounds and size.
    // Each dark pixel's mark (`NONE` for paper).
    const NONE: u32 = u32::MAX;
    let mut owner = vec![NONE; w * h];
    let mut marks: Vec<[usize; 5]> = vec![]; // x0, y0, x1, y1, pixels
    let mut stack = vec![];
    for start in 0..w * h {
        if !dark[start] || owner[start] != NONE {
            continue;
        }
        let id = marks.len() as u32;
        owner[start] = id;
        stack.push(start);
        let mut m = [usize::MAX, usize::MAX, 0, 0, 0];
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            m = [m[0].min(x), m[1].min(y), m[2].max(x + 1), m[3].max(y + 1), m[4] + 1];
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let j = ny as usize * w + nx as usize;
                    if dark[j] && owner[j] == NONE {
                        owner[j] = id;
                        stack.push(j);
                    }
                }
            }
        }
        marks.push(m);
    }
    let on_paper = |m: &[usize; 5]| {
        let (mw, mh) = ((m[2] - m[0]) as f64, (m[3] - m[1]) as f64);
        let r = (mh.min(mw) / 3.0).max(2.0) as usize;
        let (x0, y0, x1, y1) = (m[0].saturating_sub(r), m[1].saturating_sub(r), (m[2] + r).min(w), (m[3] + r).min(h));
        let (mut paper, mut all) = (0, 0);
        for y in y0..y1 {
            for x in x0..x1 {
                if x >= m[0] && x < m[2] && y >= m[1] && y < m[3] {
                    continue;
                }
                all += 1;
                paper += light(x, y) as usize;
            }
        }
        all > 0 && paper as f64 >= PAPER_AROUND * all as f64
    };
    // Letters: the size of lettering, with paper around them. Smaller marks
    // on paper (dots, commas, the bits of an ellipsis) may join a block
    // later but do not start one.
    let (mut letters, mut small): (Vec<[usize; 5]>, Vec<[usize; 5]>) = (vec![], vec![]);
    // Marks that are lettering (letters and the small marks beside them);
    // the rest are outlines and art.
    let mut lettering = vec![false; marks.len()];
    for (id, m) in marks.into_iter().enumerate() {
        let (mw, mh) = ((m[2] - m[0]) as f64, (m[3] - m[1]) as f64);
        let size = if vertical { mw } else { mh };
        if size > hi || mw > 12.0 * mh || mh > 12.0 * mw || mw.max(mh) < 2.0 || !on_paper(&m) {
            continue;
        }
        if size < lo {
            small.push(m);
            lettering[id] = true;
            continue;
        }
        let fill = m[4] as f64 / (mw * mh);
        if (0.08..=0.85).contains(&fill) {
            letters.push(m);
            lettering[id] = true;
        }
    }
    // Two letters with a balloon's outline (or other art) between them are
    // two texts: lettering in a balloon and lettering on the art beside it.
    let parted = |p: &[usize; 5], q: &[usize; 5]| {
        let (ax, ay) = ((p[0] + p[2]) as f64 / 2.0, (p[1] + p[3]) as f64 / 2.0);
        let (bx, by) = ((q[0] + q[2]) as f64 / 2.0, (q[1] + q[3]) as f64 / 2.0);
        let n = (bx - ax).abs().max((by - ay).abs()).ceil() as usize;
        (0..=n).any(|k| {
            let f = k as f64 / n.max(1) as f64;
            let (x, y) = ((ax + f * (bx - ax)) as usize, (ay + f * (by - ay)) as usize);
            let o = owner[y.min(h - 1) * w + x.min(w - 1)];
            o != NONE && !lettering[o as usize]
        })
    };
    // Letters near each other, of about one size, make a block.
    let n = letters.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let size = |m: &[usize; 5]| if vertical { (m[2] - m[0]) as f64 } else { (m[3] - m[1]) as f64 };
    // How much letter sizes in one text differ: capitals barely; kana and
    // kanji more (a small ゃ beside a full-width 漢).
    let spread = if vertical { 2.2 } else { 1.6 };
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| letters[i][1]);
    for a in 0..n {
        let i = order[a];
        for &j in &order[a + 1..] {
            let (p, q) = (&letters[i], &letters[j]);
            let s = size(p).max(size(q));
            if q[1] as f64 > p[3] as f64 + 0.7 * s {
                break;
            }
            if size(p).max(size(q)) > spread * size(p).min(size(q)) {
                continue;
            }
            let (gx, gy) = if vertical { (0.7 * s, 0.9 * s) } else { (0.9 * s, 0.7 * s) };
            if (p[0] as f64) < q[2] as f64 + gx && (q[0] as f64) < p[2] as f64 + gx && (p[1] as f64) < q[3] as f64 + gy && (q[1] as f64) < p[3] as f64 + gy && !parted(p, q) {
                let (ri, rj) = (root(&mut parent, i), root(&mut parent, j));
                parent[ri] = rj;
            }
        }
    }
    let mut groups: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    for i in 0..n {
        let r = root(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    let (mut out, mut loose): (Vec<Block>, Vec<([f64; 4], usize)>) = (vec![], vec![]);
    for g in groups.into_values() {
        let mut b = [f64::MAX, f64::MAX, 0.0f64, 0.0f64];
        let mut sizes: Vec<f64> = vec![];
        for &i in &g {
            let m = &letters[i];
            b = [b[0].min(m[0] as f64), b[1].min(m[1] as f64), b[2].max(m[2] as f64), b[3].max(m[3] as f64)];
            sizes.push(size(m));
        }
        if g.len() < 2 {
            loose.push((b, 1));
            continue;
        }
        sizes.sort_by(f64::total_cmp);
        out.push(Block { bounds: b, letters: g.len(), glyph: sizes[sizes.len() / 2], in_balloon: false });
    }
    // In page order, so which block takes a shared mark never depends on
    // the order the groups came out in.
    out.sort_by(|a, b| (a.bounds[1], a.bounds[0]).partial_cmp(&(b.bounds[1], b.bounds[0])).unwrap());
    // A lone letter or a small mark beside a block is part of it ("13...?"
    // is two letters, three dots and a question mark), one beside another
    // so an ellipsis joins dot by dot, but no further than `ABSORB` letters
    // from the block's own letters: the dots of a screentone are small
    // marks on paper too, and would otherwise join up across a panel.
    let mut loose: Vec<([f64; 4], usize, [usize; 5])> = loose.into_iter().map(|(b, n)| (b, n, [b[0] as usize, b[1] as usize, b[2] as usize, b[3] as usize, 0])).collect();
    loose.extend(small.iter().map(|m| ([m[0] as f64, m[1] as f64, m[2] as f64, m[3] as f64], 0, *m)));
    let mut taken = vec![false; loose.len()];
    for b in &mut out {
        let (gx, gy) = if vertical { (0.7 * b.glyph, 0.9 * b.glyph) } else { (0.9 * b.glyph, 0.7 * b.glyph) };
        let a = ABSORB * b.glyph;
        let own = b.bounds;
        let near = |m: [f64; 4]| m[0] >= own[0] - a && m[2] <= own[2] + a && m[1] >= own[1] - a && m[3] <= own[3] + a;
        // Nor across the balloon's outline (the dots of a screentone round it).
        let (cx, cy) = (((own[0] + own[2]) / 2.0) as usize, ((own[1] + own[3]) / 2.0) as usize);
        let centre = [cx, cy, cx + 1, cy + 1, 0];
        loop {
            let mut grew = false;
            for (k, &(m, n, mark)) in loose.iter().enumerate() {
                let r = b.bounds;
                if !taken[k] && near(m) && !parted(&mark, &centre) && m[0] < r[2] + gx && r[0] - gx < m[2] && m[1] < r[3] + gy && r[1] - gy < m[3] {
                    taken[k] = true;
                    grew = true;
                    b.bounds = [r[0].min(m[0]), r[1].min(m[1]), r[2].max(m[2]), r[3].max(m[3])];
                    b.letters += n;
                }
            }
            if !grew {
                break;
            }
        }
    }
    for b in &mut out {
        b.in_balloon = find(page, b.bounds, b.glyph, &[]).is_some();
    }
    out
}

/// How many letter-sized dark marks lie wholly inside `r` (picture pixels):
/// whether a box Gemini gave holds lettering at all, or only the art (a
/// shout fused with the panel frame, hair under it).
pub fn marks_in(page: &Page, r: [f64; 4]) -> usize {
    let x0 = r[0].max(0.0) as usize;
    let y0 = r[1].max(0.0) as usize;
    let x1 = (r[2].max(0.0) as usize).min(page.w as usize);
    let y1 = (r[3].max(0.0) as usize).min(page.h as usize);
    if x1 <= x0 || y1 <= y0 {
        return 0;
    }
    let (bw, bh) = (x1 - x0, y1 - y0);
    let hi = LETTER_MAX * page.w as f64;
    let dark = |x: usize, y: usize| page.px[y * page.w as usize + x] < INK;
    let mut seen = vec![false; bw * bh];
    let mut n = 0;
    for start in 0..bw * bh {
        if seen[start] || !dark(x0 + start % bw, y0 + start / bw) {
            continue;
        }
        seen[start] = true;
        let mut stack = vec![start];
        let (mut lo_x, mut lo_y, mut hi_x, mut hi_y, mut edge) = (bw, bh, 0, 0, false);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % bw, i / bw);
            (lo_x, lo_y, hi_x, hi_y) = (lo_x.min(x), lo_y.min(y), hi_x.max(x), hi_y.max(y));
            edge |= x == 0 || y == 0 || x + 1 == bw || y + 1 == bh;
            for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)] {
                let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                if nx < 0 || ny < 0 || nx >= bw as i64 || ny >= bh as i64 {
                    continue;
                }
                let j = ny as usize * bw + nx as usize;
                if !seen[j] && dark(x0 + nx as usize, y0 + ny as usize) {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
        let (mw, mh) = ((hi_x - lo_x + 1) as f64, (hi_y - lo_y + 1) as f64);
        if !edge && mw.max(mh) <= hi && mw.max(mh) >= 2.0 && mw <= 12.0 * mh && mh <= 12.0 * mw {
            n += 1;
        }
    }
    n
}

/// Half a letter's stroke, in letters: a speed line thinner than a stroke
/// is worn away from a letter it touches, the letter left.
const STROKE: f64 = 0.05;

/// How tall, in letters, a mark of letters touching each other may be: a
/// line of them, give or take.
const LINE_TALL: f64 = 1.5;

/// How far round a mark's bounds (pixels) the paper round lettering is
/// looked for, and the share of that ring it takes.
const EDGE_RING: usize = 2;
const PAPER_RING: f64 = 0.5;

/// The white rim lettered round each letter of lettering with no balloon, in
/// letters: how far its cover reaches past the strokes.
const RIM: f64 = 0.2;

/// Lettering with no balloon round it, covered where it is drawn rather than
/// with a box: the letter-sized dark marks reaching into `r`, sought a letter
/// beyond it (a line its edge cuts through is still lettering; art running in
/// from further out is left), each its own rectangle grown by its rim. The
/// art between letters, words and lines stays. A mark is lettering with
/// paper round it, or lying in one of `lettered` (blocks found on the page,
/// on a pattern that rings them: ch256 p21, "4...?" on a net). Empty when `r` holds no such marks (white letters
/// outlined on dark art), and the caller covers `r` instead.
pub fn silhouette(page: &Page, r: [f64; 4], glyph: f64, lettered: &[[f64; 4]]) -> Vec<[i64; 4]> {
    let (x0, y0) = ((r[0] - glyph).max(0.0) as usize, (r[1] - glyph).max(0.0) as usize);
    let (x1, y1) = (((r[2] + glyph).max(0.0) as usize).min(page.w as usize), ((r[3] + glyph).max(0.0) as usize).min(page.h as usize));
    if x1 <= x0 || y1 <= y0 {
        return vec![];
    }
    let (bw, bh) = (x1 - x0, y1 - y0);
    let hi = LETTER_MAX * page.w as f64;
    let dark: Vec<bool> = (0..bw * bh).map(|i| page.px[(y0 + i / bw) * page.w as usize + x0 + i % bw] < INK).collect();
    let mut keep = vec![false; bw * bh];
    // Paper (or a letter's white edging) right round a mark's bounds, as
    // there is round lettering and not round grass tufts on a gray tone
    // (ch258 p15, over "Ah...").
    let papered = |[lo_x, lo_y, hi_x, hi_y]: [usize; 4]| {
        let (ax, ay, bx, by) = (lo_x.saturating_sub(EDGE_RING), lo_y.saturating_sub(EDGE_RING), (hi_x + EDGE_RING).min(bw - 1), (hi_y + EDGE_RING).min(bh - 1));
        let ring: Vec<u8> = (ay..=by).flat_map(|y| (ax..=bx).map(move |x| (x, y))).filter(|&(x, y)| x < lo_x || x > hi_x || y < lo_y || y > hi_y).map(|(x, y)| page.px[(y0 + y) * page.w as usize + x0 + x]).collect();
        let (cx, cy) = ((x0 + lo_x + x0 + hi_x) as f64 / 2.0, (y0 + lo_y + y0 + hi_y) as f64 / 2.0);
        lettered.iter().any(|b| cx >= b[0] && cx <= b[2] && cy >= b[1] && cy <= b[3]) || ring.iter().filter(|&&v| v >= LIGHT).count() as f64 >= PAPER_RING * ring.len() as f64
    };
    // The marks among the pixels `on` from `lo` to `hi` across, reaching
    // into `r` and not running in from the region's edge.
    let letters = |on: &[bool], lo: f64, hi: f64, paper: bool, keep: &mut Vec<bool>| {
        for (mark, b, edge) in pieces(on, bw, bh) {
            let [lo_x, lo_y, hi_x, hi_y] = b;
            let size = ((hi_x - lo_x + 1).max(hi_y - lo_y + 1)) as f64;
            let into = ((x0 + lo_x) as f64) < r[2] && ((x0 + hi_x + 1) as f64) > r[0] && ((y0 + lo_y) as f64) < r[3] && ((y0 + hi_y + 1) as f64) > r[1];
            if !edge && into && size <= hi && size >= lo && (!paper || papered(b)) {
                mark.into_iter().for_each(|i| keep[i] = true);
            }
        }
    };
    letters(&dark, 2.0, hi, true, &mut keep);
    // Letters touching each other are too big a mark to keep (ch257 p11,
    // "TH" in "THE"). The thick strokes of a mark a line of letters tall,
    // thin lines worn away from them, are sought again, each from a third
    // of a letter to a letter and a half across. A mark taller is art: a
    // heavy balloon outline (p11, right of "HAKARI") or hatched hair (ch256
    // p10, under "YODEL-AY-HEE!"), its strands letter-sized.
    let rest: Vec<bool> = (0..bw * bh).map(|i| dark[i] && !keep[i]).collect();
    let thick = opened(&rest, bw, bh, ((STROKE * glyph).round() as usize).max(1));
    let mut sought = vec![false; bw * bh];
    for (mark, [lo_x, lo_y, hi_x, hi_y], _) in pieces(&rest, bw, bh) {
        let across = ((hi_x - lo_x + 1).min(hi_y - lo_y + 1)) as f64;
        if across <= LINE_TALL * glyph {
            mark.into_iter().filter(|&i| thick[i]).for_each(|i| sought[i] = true);
        }
    }
    letters(&sought, glyph / 3.0, LINE_TALL * glyph, false, &mut keep);
    // Each letter's own rectangle, grown by its rim: not a band across a
    // line, which takes in the art between letters and lines (ch256 p10,
    // hair under "YODEL-AY-HEE!").
    let rim = ((RIM * glyph).round() as usize).max(2);
    pieces(&keep, bw, bh)
        .into_iter()
        .map(|(_, b, _)| [(x0 + b[0]).saturating_sub(rim).max(x0) as i64, (y0 + b[1]).saturating_sub(rim).max(y0) as i64, (x0 + b[2] + 1 + rim).min(x1) as i64, (y0 + b[3] + 1 + rim).min(y1) as i64])
        .collect()
}

/// The 8-connected pieces of the pixels `on` in a `w` × `h` region: each
/// one's pixels, bounds (`x0, y0, x1, y1`, inclusive) and whether it meets
/// the region's edge.
pub fn pieces(on: &[bool], w: usize, h: usize) -> Vec<(Vec<usize>, [usize; 4], bool)> {
    let mut seen = vec![false; w * h];
    let mut out = vec![];
    for start in 0..w * h {
        if seen[start] || !on[start] {
            continue;
        }
        seen[start] = true;
        let (mut stack, mut mark) = (vec![start], vec![]);
        let (mut b, mut edge) = ([w, h, 0, 0], false);
        while let Some(i) = stack.pop() {
            mark.push(i);
            let (x, y) = (i % w, i / w);
            b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
            edge |= x == 0 || y == 0 || x + 1 == w || y + 1 == h;
            for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)] {
                let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                    continue;
                }
                let j = ny as usize * w + nx as usize;
                if !seen[j] && on[j] {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
        out.push((mark, b, edge));
    }
    out
}

/// The pixels `on` that lie within `r` of a square `2r + 1` wide wholly
/// `on`: strokes that thick or thicker, without the thinner lines running
/// from them.
pub fn opened(on: &[bool], w: usize, h: usize, r: usize) -> Vec<bool> {
    let r = r as i64;
    let all = |i: usize, test: &dyn Fn(usize) -> bool| {
        let (x, y) = (i % w, i / w);
        (-r..=r).all(|dy| (-r..=r).all(|dx| {
            let (nx, ny) = (x as i64 + dx, y as i64 + dy);
            nx >= 0 && ny >= 0 && nx < w as i64 && ny < h as i64 && test(ny as usize * w + nx as usize)
        }))
    };
    let core: Vec<bool> = (0..w * h).map(|i| on[i] && all(i, &|j| on[j])).collect();
    (0..w * h).map(|i| on[i] && !all(i, &|j| !core[j])).collect()
}

/// How far, in letters, marks may join a block beyond its own letters.
const ABSORB: f64 = 2.0;

/// Which block each of Gemini's items sits on: `items` are its text boxes
/// (picture pixels) and how many letters its source text has. Nearest first,
/// then trades that lower the total cost; one block per item, and an item
/// with no block near enough gets none.
pub fn assign(items: &[([f64; 4], usize)], blocks: &[Block], page_w: u32) -> Vec<Option<usize>> {
    let centre = |b: [f64; 4]| ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
    // The page's usual lettering size, from its larger blocks: marks much
    // smaller than that (a face's eyes, a pattern) are seldom dialogue.
    let mut sizes: Vec<f64> = blocks.iter().filter(|b| b.letters >= 6).map(|b| b.glyph).collect();
    sizes.sort_by(f64::total_cmp);
    let usual = sizes.get(sizes.len() / 2).copied();
    // How much of `r` lies in Gemini's box `t`, drawn loosely: a tenth of
    // the box round it still counts (ch256 p4, the "34!" below its box).
    let held = |t: [f64; 4], r: [f64; 4]| {
        let (gx, gy) = (MATCH_LOOSE * (t[2] - t[0]), MATCH_LOOSE * (t[3] - t[1]));
        let w = ((t[2] + gx).min(r[2]) - (t[0] - gx).max(r[0])).max(0.0);
        w * ((t[3] + gy).min(r[3]) - (t[1] - gy).max(r[1])).max(0.0) / ((r[2] - r[0]) * (r[3] - r[1])).max(1.0)
    };
    let mut pairs = vec![];
    for (i, &(t, chars)) in items.iter().enumerate() {
        let (ix, iy) = centre(t);
        // The largest lettering in the box: smaller marks beside it there
        // are its dots, not the line (ch256 p21, "..." in "3... 4...?").
        let largest = blocks.iter().filter(|b| held(t, b.bounds) >= 0.5).map(|b| b.glyph).fold(0.0, f64::max);
        for (j, b) in blocks.iter().enumerate() {
            let (bx, by) = centre(b.bounds);
            let d = ((ix - bx).powi(2) + (iy - by).powi(2)).sqrt() / page_w as f64;
            let count = ((b.letters as f64 + 1.0) / (chars as f64 + 1.0)).ln().abs();
            let small = usual.map_or(0.0, |u| (0.7 * u / b.glyph).ln().max(0.0));
            let r = b.bounds;
            let inter = (t[2].min(r[2]) - t[0].max(r[0])).max(0.0) * (t[3].min(r[3]) - t[1].max(r[1])).max(0.0);
            // Under the box: filling it, or lying within it with the item's
            // letters (a short shout in a box Gemini drew large, ch256 p7
            // "WH...?!"; not two specks in a caption's box, ch256 p1).
            let within = inter / ((r[2] - r[0]) * (r[3] - r[1])).max(1.0) * (b.letters as f64 / chars.max(1) as f64).min(1.0);
            let filled = (inter / ((t[2] - t[0]) * (t[3] - t[1])).max(1.0)).max(within);
            let bare = if b.in_balloon { 0.0 } else { MATCH_BARE * (1.0 - filled) };
            // Lettering lying within the box, the largest there, is the
            // item's (one with letters; "...!" has none to tell), though it
            // has more letters or smaller ones than the item reads: a reading
            // lettered over the line ("san yon" over "34!", ch256 p4, whose
            // "34!" went to the "7!" in the next balloon). Not with fewer:
            // that is another line, under a box Gemini set a balloon over
            // (ch258 p14, "THE RIVER STYX-" under the box of "THERE...").
            let theirs = if chars > 0 && b.letters >= chars && b.glyph * 1.6 >= largest { held(t, r) } else { 0.0 };
            let cost = d + (MATCH_COUNT * count + MATCH_SMALL * small) * (1.0 - theirs) + bare;
            if cost <= MATCH_MAX {
                pairs.push((cost, i, j));
            }
        }
    }
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = vec![None; items.len()];
    let mut taken = vec![false; blocks.len()];
    let cost: std::collections::HashMap<(usize, usize), f64> = pairs.iter().map(|&(c, i, j)| ((i, j), c)).collect();
    for (_, i, j) in pairs {
        if out[i].is_none() && !taken[j] {
            out[i] = Some(j);
            taken[j] = true;
        }
    }
    // Nearest first goes wrong when Gemini put every box of a row a balloon
    // over: the first item takes its neighbour's lettering, and the
    // neighbour is left the one its box is furthest from (ch257 p4, "NOBODY
    // INVITED YOU." and "OH, RENTAROU-KUN..."). Two items trade blocks
    // whenever that costs less in all.
    let mut traded = true;
    while traded {
        traded = false;
        for i in 0..out.len() {
            for k in i + 1..out.len() {
                let (Some(a), Some(b)) = (out[i], out[k]) else { continue };
                let (Some(&ia), Some(&kb), Some(&ib), Some(&ka)) = (cost.get(&(i, a)), cost.get(&(k, b)), cost.get(&(i, b)), cost.get(&(k, a))) else { continue };
                if ib + ka < ia + kb - 1e-9 {
                    (out[i], out[k]) = (Some(b), Some(a));
                    traded = true;
                }
            }
        }
    }
    out
}

/// Gemini's box for an item [`assign`] left without lettering, moved onto
/// the line it missed: when the box meets no block at all and a word or
/// more nobody took lies within [`REACH`] letters of it, the box moves just
/// far enough to hold that line, and the line is the item's (ch257 p10, a
/// footnote's box two lines above it; its cover went to the frills there).
/// Of lines equally near, the longest.
pub fn nudge(items: &mut [([f64; 4], usize)], blocks: &[Block], on: &mut [Option<usize>]) {
    let meets = |t: [f64; 4], r: [f64; 4]| t[0] < r[2] && r[0] < t[2] && t[1] < r[3] && r[1] < t[3];
    for i in 0..items.len() {
        let t = items[i].0;
        if on[i].is_some() || blocks.iter().any(|b| meets(t, b.bounds)) {
            continue;
        }
        let gap = |r: [f64; 4]| (r[0] - t[2]).max(t[0] - r[2]).max(r[1] - t[3]).max(t[1] - r[3]);
        let near = blocks
            .iter()
            .enumerate()
            .filter(|&(j, b)| !on.contains(&Some(j)) && b.letters >= WORD_LETTERS && gap(b.bounds) <= REACH * b.glyph)
            .min_by(|a, b| gap(a.1.bounds).total_cmp(&gap(b.1.bounds)).then(b.1.letters.cmp(&a.1.letters)));
        let Some((j, b)) = near else { continue };
        let r = b.bounds;
        let dx = if r[0] >= t[2] { r[2] - t[2] } else if r[2] <= t[0] { r[0] - t[0] } else { 0.0 };
        let dy = if r[1] >= t[3] { r[3] - t[3] } else if r[3] <= t[1] { r[1] - t[1] } else { 0.0 };
        items[i].0 = [t[0] + dx, t[1] + dy, t[2] + dx, t[3] + dy];
        on[i] = Some(j);
    }
}

/// Each item's lettering: its block, joined by blocks no item took that
/// are lettered at its size and either lie almost wholly inside the item's
/// box or, half inside it or reaching into it within a few letters of the
/// lettering, bring the letter count nearer the item's source text; or that
/// lie inside the box touching the lettering, at any size. Gemini gives two texts
/// lettered apart as one item ("IGNORE IT!" above "JUST AS LONG AS...",
/// "WHAT DO I..." beside "OH, I KNOW!"). The bounds of each piece, its own
/// block first; none for an item with no block.
pub fn gather(items: &[([f64; 4], usize)], blocks: &[Block], on: &[Option<usize>]) -> Vec<Vec<[f64; 4]>> {
    // A piece of lettering lying within another item's block (two letters
    // the block's grouping left apart) is that item's: never another's to
    // gather (ch256 p17, "WELL" in "WELL, UMM..." gathered by the caption
    // beside it, whose cover then pushed this balloon's aside).
    let within = |r: [f64; 4], o: [f64; 4]| r[0] >= o[0] && r[1] >= o[1] && r[2] <= o[2] && r[3] <= o[3];
    let mut taken: Vec<bool> = blocks.iter().enumerate().map(|(j, b)| on.iter().flatten().any(|&k| k == j || within(b.bounds, blocks[k].bounds))).collect();
    let mut out = vec![];
    for (&(t, chars), o) in items.iter().zip(on) {
        let Some(own) = o.map(|j| &blocks[j]) else {
            out.push(vec![]);
            continue;
        };
        let (mut b, mut letters, mut pieces) = (own.bounds, own.letters, vec![own.bounds]);
        let miss = |n: usize| ((n as f64 + 1.0) / (chars as f64 + 1.0)).ln().abs();
        loop {
            let reach = REACH * own.glyph;
            let next = blocks.iter().enumerate().find(|&(j, other)| {
                let r = other.bounds;
                let inter = (t[2].min(r[2]) - t[0].max(r[0])).max(0.0) * (t[3].min(r[3]) - t[1].max(r[1])).max(0.0);
                let inside = inter >= 0.8 * (r[2] - r[0]) * (r[3] - r[1]);
                let gap = (r[0] - b[2]).max(b[0] - r[2]).max(r[1] - b[3]).max(b[1] - r[3]);
                // Within reach of the lettering gathered so far, and on
                // Gemini's box: not lettering met one join after another
                // across the panel (ch257 p8, a sound effect and then the
                // next panel's shout). A second text is a word or more;
                // a few marks beside the box are more often the art (ch257
                // p19, hair strokes with the "G" of "GAH!").
                // Or half under the box at any distance: the next balloon of
                // a box Gemini drew round two (ch258 p4, "NUREMI SUIMU, YEAR
                // 3 CLASS 5." under "I'M THE CAPTAIN OF THE SWIM CLUB.").
                let near = gap <= reach && inter > 0.0 || inter >= 0.5 * (r[2] - r[0]) * (r[3] - r[1]);
                let closer = near && other.letters >= WORD_LETTERS && miss(letters + other.letters) < miss(letters);
                let alike = other.glyph.max(own.glyph) <= 1.6 * other.glyph.min(own.glyph);
                // Lettering of another size inside the box and touching the
                // item's is the same line: a reading lettered small over it
                // ("nana" over "7!", ch256 p4).
                let touching = inside && gap <= other.glyph.min(own.glyph) && other.in_balloon && other.letters >= WORD_LETTERS;
                !taken[j] && other.letters >= 3 && (alike && (inside || closer) || touching)
            });
            let Some((j, other)) = next else { break };
            taken[j] = true;
            letters += other.letters;
            let r = other.bounds;
            b = [b[0].min(r[0]), b[1].min(r[1]), b[2].max(r[2]), b[3].max(r[3])];
            pieces.push(r);
        }
        out.push(pieces);
    }
    out
}

/// The fewest letters a text gathered from beside an item's box holds: a
/// word ("◆ Plung" ahead of a caption's box, ch258 p19).
pub const WORD_LETTERS: usize = 4;

/// Matching cost: distance in page widths, plus this much per unit of the
/// log ratio of letter counts; beyond `MATCH_MAX`, no match.
const MATCH_COUNT: f64 = 0.25;
/// Per unit of the log ratio by which a block's letters fall short of 70%
/// of the page's usual lettering size.
const MATCH_SMALL: f64 = 0.5;
const MATCH_MAX: f64 = 0.6;
/// How far past Gemini's box, as a share of its size, lettering still lies
/// within it.
const MATCH_LOOSE: f64 = 0.1;
/// For a block with no balloon around it, less the share of Gemini's box it
/// fills (or of itself, and of the item's letters, inside that box):
/// dialogue Gemini placed off its balloon goes to the lettering in a
/// balloon a little further away, not to marks on the art its box grazes,
/// while a shout lettered on the art right under Gemini's box keeps it (and
/// does not go to a pair of eyes whose whites look like a balloon).
const MATCH_BARE: f64 = 0.3;

/// The original letter size: the median height of the lettering's lines
/// (their width, for vertical text), from where its strokes lie.
pub fn glyph(strokes: &[(i64, i64)], vertical: bool) -> Option<f64> {
    let along = |&(x, y): &(i64, i64)| if vertical { x } else { y };
    let lo = strokes.iter().map(along).min()?;
    let hi = strokes.iter().map(along).max()?;
    let mut filled = vec![false; (hi - lo + 1) as usize];
    for p in strokes {
        filled[(along(p) - lo) as usize] = true;
    }
    let mut lines = vec![];
    let mut run = 0;
    for f in filled.into_iter().chain([false]) {
        if f {
            run += 1;
        } else if run > 0 {
            lines.push(run);
            run = 0;
        }
    }
    // Punctuation alone on a row (a comma, the dots of an ellipsis) is not a line.
    let tallest = *lines.iter().max()?;
    let mut lines: Vec<i64> = lines.into_iter().filter(|&n| n * 3 >= tallest).collect();
    lines.sort_unstable();
    let mid = lines.len() / 2;
    Some(if lines.len() % 2 == 1 { lines[mid] as f64 } else { (lines[mid - 1] + lines[mid]) as f64 / 2.0 })
}

/// Squared distance from a pixel's centre to a rectangle.
fn rect_dist2(r: [f64; 4], x: i64, y: i64) -> f64 {
    let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
    let dx = (r[0] - px).max(0.0).max(px - r[2]);
    let dy = (r[1] - py).max(0.0).max(py - r[3]);
    dx * dx + dy * dy
}

/// One row band of the cover: columns `[l, r)`, rows `[top, bottom)`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Step {
    l: i64,
    r: i64,
    top: i64,
    bottom: i64,
}

/// Rows merged into steps while no row loses more than `slack` pixels a side.
fn steps(rows: &[(i64, i64, i64)], slack: i32) -> Vec<Step> {
    let slack = slack as i64;
    let mut out: Vec<Step> = vec![];
    let mut first = (0, 0); // the step's widest row extent, to bound what it gives up
    for &(y, l, r) in rows {
        if let Some(s) = out.last_mut() {
            let (nl, nr) = (s.l.max(l), s.r.min(r));
            let (wl, wr) = (first.0.min(l), first.1.max(r));
            if s.bottom == y && nl - wl <= slack && wr - nr <= slack && nr > nl {
                *s = Step { l: nl, r: nr, top: s.top, bottom: y + 1 };
                first = (wl, wr);
                continue;
            }
        }
        out.push(Step { l, r, top: y, bottom: y + 1 });
        first = (l, r);
    }
    out
}

fn covers(steps: &[Step], ink: &[(i64, i64)]) -> bool {
    !steps.is_empty() && ink.iter().all(|&(x, y)| steps.iter().any(|s| y >= s.top && y < s.bottom && x >= s.l && x < s.r))
}

/// The steps' outline, clockwise: down the right side, up the left.
fn outline(steps: &[Step]) -> Vec<[i64; 2]> {
    let mut pts = vec![];
    for s in steps {
        pts.push([s.r, s.top]);
        pts.push([s.r, s.bottom]);
    }
    for s in steps.iter().rev() {
        pts.push([s.l, s.bottom]);
        pts.push([s.l, s.top]);
    }
    // Points in a straight line add nothing.
    let mut out: Vec<[i64; 2]> = vec![];
    for p in pts {
        if out.last() == Some(&p) {
            continue;
        }
        if out.len() >= 2 {
            let (a, b) = (out[out.len() - 2], out[out.len() - 1]);
            if (a[0] == b[0] && b[0] == p[0]) || (a[1] == b[1] && b[1] == p[1]) {
                out.pop();
            }
        }
        out.push(p);
    }
    out
}

/// The largest rectangle inside consecutive steps, centred on `at`: as far
/// up as down from it and as far left as right, so the Korean sits where
/// the original lettering sat.
fn largest_rect(steps: &[Step], at: (i64, i64)) -> Option<[i64; 4]> {
    let (cx, cy) = at;
    let mid = steps.iter().position(|s| cy >= s.top && cy < s.bottom)?;
    let mut best: Option<([i64; 4], i64)> = None;
    let (mut l, mut r) = (i64::MIN, i64::MAX);
    // Grow one step at a time on whichever side keeps the rectangle centred.
    let (mut up, mut down) = (mid, mid);
    loop {
        l = l.max(steps[up].l).max(steps[down].l);
        r = r.min(steps[up].r).min(steps[down].r);
        let half_w = (cx - l).min(r - cx);
        let half_h = (cy - steps[up].top).min(steps[down].bottom - cy);
        if half_w <= 0 {
            break;
        }
        // A step starting at the centre row has no height above it yet: the
        // rectangle grows on (a rounded cover is many one-row steps).
        let area = half_w * half_h;
        if half_h > 0 && best.is_none_or(|(_, a)| area > a) {
            best = Some(([cx - half_w, cy - half_h, cx + half_w, cy + half_h], area));
        }
        let can_up = up > 0 && steps[up - 1].bottom == steps[up].top;
        let can_down = down + 1 < steps.len() && steps[down + 1].top == steps[down].bottom;
        match (can_up, can_down) {
            (false, false) => break,
            (true, false) => up -= 1,
            (false, true) => down += 1,
            (true, true) if cy - steps[up].top <= steps[down].bottom - cy => up -= 1,
            (true, true) => down += 1,
        }
    }
    best.map(|(r, _)| r)
}

/// A window of the page, as a flat grid.
struct Grid {
    x0: i64,
    y0: i64,
    w: i64,
    h: i64,
}

impl Grid {
    fn new(r: [i64; 4]) -> Grid {
        Grid { x0: r[0], y0: r[1], w: (r[2] - r[0]).max(0), h: (r[3] - r[1]).max(0) }
    }

    fn xy(&self, i: usize) -> (i64, i64) {
        (self.x0 + i as i64 % self.w, self.y0 + i as i64 / self.w)
    }

    fn cells(&self) -> impl Iterator<Item = (i64, i64)> + '_ {
        (0..self.w * self.h).map(|i| self.xy(i as usize))
    }

    /// Cells whose centre is inside `r`.
    fn inside(&self, r: [f64; 4]) -> impl Iterator<Item = usize> + '_ {
        let x0 = ((r[0] - 0.5).ceil() as i64).max(self.x0);
        let x1 = ((r[2] - 0.5).floor() as i64).min(self.x0 + self.w - 1);
        let y0 = ((r[1] - 0.5).ceil() as i64).max(self.y0);
        let y1 = ((r[3] - 0.5).floor() as i64).min(self.y0 + self.h - 1);
        (y0..=y1).flat_map(move |y| (x0..=x1).map(move |x| ((y - self.y0) * self.w + x - self.x0) as usize))
    }

    fn neighbours(&self, i: usize) -> impl Iterator<Item = usize> {
        let (w, n) = (self.w as usize, (self.w * self.h) as usize);
        let x = i % w;
        [(x > 0).then(|| i - 1), (x + 1 < w).then(|| i + 1), i.checked_sub(w), (i + w < n).then_some(i + w)].into_iter().flatten()
    }

    /// Connected regions of `on` (4-neighbours); 0 for cells not on.
    fn components(&self, on: &[bool]) -> Vec<u32> {
        let mut label = vec![0u32; on.len()];
        let mut next = 0;
        let mut stack = vec![];
        for start in 0..on.len() {
            if !on[start] || label[start] != 0 {
                continue;
            }
            next += 1;
            label[start] = next;
            stack.push(start);
            while let Some(i) = stack.pop() {
                for j in self.neighbours(i) {
                    if on[j] && label[j] == 0 {
                        label[j] = next;
                        stack.push(j);
                    }
                }
            }
        }
        label
    }

    fn edge(&self) -> impl Iterator<Item = usize> + '_ {
        let (w, h) = (self.w as usize, self.h as usize);
        (0..w).flat_map(move |x| [x, (h - 1) * w + x]).chain((0..h).flat_map(move |y| [y * w, y * w + w - 1]))
    }

    /// Cells not in `wall` that the window's edge, or a cell of `also`,
    /// reaches without crossing it.
    fn reach_from(&self, wall: &[bool], also: &[bool]) -> Vec<bool> {
        let mut seen = vec![false; wall.len()];
        let mut stack: Vec<usize> = self.edge().filter(|&i| !wall[i]).chain((0..wall.len()).filter(|&i| also[i] && !wall[i])).collect();
        for &i in &stack {
            seen[i] = true;
        }
        while let Some(i) = stack.pop() {
            for j in self.neighbours(i) {
                if !wall[j] && !seen[j] {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
        seen
    }

    /// Each cell's distance in pixels to the nearest cell outside `on` (or
    /// the window's edge): a 3-4 chamfer, so diagonals count about √2.
    fn distance(&self, on: &[bool]) -> Vec<i32> {
        let (w, h) = (self.w as usize, self.h as usize);
        let big = i32::MAX / 4;
        let mut d: Vec<i32> = on.iter().map(|&o| if o { big } else { 0 }).collect();
        let get = |d: &Vec<i32>, x: isize, y: isize| if x < 0 || y < 0 || x >= w as isize || y >= h as isize { 0 } else { d[y as usize * w + x as usize] };
        for y in 0..h as isize {
            for x in 0..w as isize {
                let i = y as usize * w + x as usize;
                if d[i] == 0 {
                    continue;
                }
                let v = [get(&d, x - 1, y) + 3, get(&d, x, y - 1) + 3, get(&d, x - 1, y - 1) + 4, get(&d, x + 1, y - 1) + 4];
                d[i] = d[i].min(v.into_iter().min().unwrap_or(big));
            }
        }
        for y in (0..h as isize).rev() {
            for x in (0..w as isize).rev() {
                let i = y as usize * w + x as usize;
                if d[i] == 0 {
                    continue;
                }
                let v = [get(&d, x + 1, y) + 3, get(&d, x, y + 1) + 3, get(&d, x + 1, y + 1) + 4, get(&d, x - 1, y + 1) + 4];
                d[i] = d[i].min(v.into_iter().min().unwrap_or(big));
            }
        }
        d.into_iter().map(|v| v / 3).collect()
    }

    /// For each row, the run of `on` around the text: around the middle of
    /// the row's own strokes, or `cx` on rows without any. `(y, l, r)`.
    fn runs(&self, on: &[bool], ink: &[usize], cx: i64) -> Vec<(i64, i64, i64)> {
        let w = self.w as usize;
        let mut row_ink: Vec<Option<(i64, i64)>> = vec![None; self.h as usize];
        for &i in ink {
            let (x, _) = self.xy(i);
            let r = &mut row_ink[i / w];
            *r = Some(r.map_or((x, x), |(a, b)| (a.min(x), b.max(x))));
        }
        let mut out = vec![];
        for (row, ri) in row_ink.iter().enumerate() {
            let at = |x: i64| on[row * w + (x - self.x0) as usize];
            let mid = ri.map_or(cx, |(a, b)| (a + b) / 2).clamp(self.x0, self.x0 + self.w - 1);
            if !at(mid) {
                continue;
            }
            let (mut l, mut r) = (mid, mid + 1);
            while l > self.x0 && at(l - 1) {
                l -= 1;
            }
            while r < self.x0 + self.w && at(r) {
                r += 1;
            }
            out.push((self.y0 + row as i64, l, r));
        }
        // Only the rows joined to the text's own: a run cut off above or
        // below (a spike of the outline) is not part of this cover.
        let Some(start) = out.iter().position(|&(y, _, _)| row_ink[(y - self.y0) as usize].is_some()) else { return out };
        let mut lo = start;
        while lo > 0 && out[lo - 1].0 + 1 == out[lo].0 && out[lo - 1].1 < out[lo].2 && out[lo].1 < out[lo - 1].2 {
            lo -= 1;
        }
        let mut hi = start;
        while hi + 1 < out.len() && out[hi + 1].0 == out[hi].0 + 1 && out[hi + 1].1 < out[hi].2 && out[hi].1 < out[hi + 1].2 {
            hi += 1;
        }
        out[lo..=hi].to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The user, on the phone (ch257 p6): "WHAT KIND OF A FATHER..." is
    /// lettered in white-edged letters on the panel's gray tone, which ran
    /// up to the next balloon; it was taken for one balloon and its white
    /// joined that balloon's. Lettering edged in white on a tone no outline
    /// closes is not in a balloon; the same lettering in a closed toned
    /// balloon (ch256 p8, "EE HEE HEE...") is.
    #[test]
    fn white_edged_lettering_on_an_open_tone_is_not_a_balloon() {
        let (w, h) = (600u32, 600u32);
        let lettered = |closed: bool| {
            let mut px = vec![228u8; (w * h) as usize];
            if closed {
                // A dark ellipse round the lettering, its inside the same tone.
                for y in 0..h as i64 {
                    for x in 0..w as i64 {
                        let e = ((x as f64 - 300.0) / 150.0).powi(2) + ((y as f64 - 300.0) / 110.0).powi(2);
                        if (0.94..1.06).contains(&e) {
                            px[(y as u32 * w + x as u32) as usize] = 0;
                        }
                    }
                }
            }
            let mut fill = |x0: usize, y0: usize, x1: usize, y1: usize, v: u8| (y0..y1).for_each(|y| (x0..x1).for_each(|x| px[y * w as usize + x] = v));
            for y in [270, 302] {
                for x in [200, 220, 240, 320, 340, 360] {
                    fill(x - 3, y - 3, x + 17, y + 19, 255);
                }
            }
            for y in [270, 302] {
                for x in [200, 220, 240, 320, 340, 360] {
                    fill(x, y, x + 14, y + 16, 0);
                }
            }
            px
        };
        let (open, closed) = (lettered(false), lettered(true));
        let text = [200.0, 270.0, 374.0, 318.0];
        assert!(find(&Page { w, h, px: &open }, text, 16.0, &[]).is_none());
        assert!(find(&Page { w, h, px: &closed }, text, 16.0, &[]).is_some());
    }

    /// The user, on the phone: lettering with no balloon was covered with a
    /// white box over the art. Its cover is its letters and their rim; the
    /// art between two words a few letters apart, and art reaching in from
    /// outside, stay as drawn.
    #[test]
    fn lettering_with_no_balloon_is_covered_letter_by_letter() {
        let (w, h) = (400u32, 200u32);
        let mut px = vec![230u8; (w * h) as usize];
        // A grass tuft's gray tone (ch258 p15, over "Ah...").
        (100..130).for_each(|y| (280..320).for_each(|x| px[y * w as usize + x] = 150));
        let mut ink = |x0: usize, y0: usize, x1: usize, y1: usize| (y0..y1).for_each(|y| (x0..x1).for_each(|x| px[y * w as usize + x] = 0));
        // Two words of three 16 px letters, 60 px apart; a hair stroke
        // running in from the left edge of the box; a line above that the
        // box's top edge cuts through (ch257 p6, "UNINSTALL").
        for x in [100, 120, 140, 220, 240, 260] {
            ink(x, 80, x + 16, 96);
            ink(x, 42, x + 16, 58);
        }
        ink(0, 120, 200, 123);
        // A letter a speed line 1 px thin runs through, across the page
        // (ch257 p11, "KA" in "HAKARI").
        ink(0, 140, 400, 141);
        ink(150, 132, 166, 148);
        // The tuft on its tone: two thin blades.
        ink(295, 110, 297, 120);
        ink(301, 110, 303, 120);
        let tuft = |lettered: &[[f64; 4]]| {
            let cover = silhouette(&Page { w, h, px: &px }, [60.0, 50.0, 320.0, 150.0], 16.0, lettered);
            cover.iter().any(|r| 300 >= r[0] && 300 < r[2] && 115 >= r[1] && 115 < r[3])
        };
        // Not lettering, unless the page's lettering was found there (a
        // net ringing "4...?", ch256 p21).
        assert!(!tuft(&[]) && tuft(&[[290.0, 105.0, 310.0, 125.0]]));
        let cover = silhouette(&Page { w, h, px: &px }, [60.0, 50.0, 320.0, 150.0], 16.0, &[]);
        let on = |x: i64, y: i64| cover.iter().any(|r| x >= r[0] && x < r[2] && y >= r[1] && y < r[3]);
        // Every stroke, and a rim past each edge.
        for x in [97, 100, 115, 135, 155, 158, 217, 275, 278] {
            assert!(on(x, 88), "x {x}");
        }
        assert!(on(130, 77) && on(130, 98) && on(108, 43) && on(268, 57) && on(158, 133) && on(165, 147));
        assert!(!on(70, 140) && !on(300, 140));
        // Not the art between the words, beyond the rim, or the hair.
        assert!(!on(190, 88) && !on(130, 70) && !on(130, 104) && !on(120, 121) && !on(70, 88));
    }

    /// A white page with an elliptical balloon (outline 3 px), centred at
    /// (cx, cy) with half-axes (a, b), and black text bars inside.
    fn page(w: u32, h: u32, balloons: &[(f64, f64, f64, f64)], bars: &[[i64; 4]]) -> Vec<u8> {
        let mut px = vec![90u8; (w * h) as usize]; // gray art
        for y in 0..h as i64 {
            for x in 0..w as i64 {
                for &(cx, cy, a, b) in balloons {
                    let e = ((x as f64 - cx) / a).powi(2) + ((y as f64 - cy) / b).powi(2);
                    let i = (y as u32 * w + x as u32) as usize;
                    if e <= 1.0 {
                        px[i] = 255;
                    } else if e <= (1.0 + 3.0 / a.min(b)).powi(2) {
                        px[i] = 0;
                    }
                }
            }
        }
        for b in bars {
            for y in b[1]..b[3] {
                for x in b[0]..b[2] {
                    px[(y as u32 * w + x as u32) as usize] = 0;
                }
            }
        }
        px
    }

    fn inside_ellipse(p: [i64; 2], (cx, cy, a, b): (f64, f64, f64, f64)) -> bool {
        ((p[0] as f64 - cx) / a).powi(2) + ((p[1] as f64 - cy) / b).powi(2) <= 1.0
    }

    /// Three lines of lettering, the middle one widest (as English is
    /// lettered), in an ellipse: the cover takes in every stroke, its corners
    /// stay inside the outline, and the letter height is read off.
    #[test]
    fn a_diamond_of_lettering_is_covered_inside_its_ellipse() {
        let e = (200.0, 150.0, 120.0, 80.0);
        let bars = [[160, 110, 240, 128], [120, 141, 280, 159], [160, 172, 240, 190]];
        let px = page(400, 300, &[e], &bars);
        let p = Page { w: 400, h: 300, px: &px };
        // Gemini's box runs short of the widest line's ends.
        let f = find(&p, [130.0, 112.0, 270.0, 188.0], 18.0, &[]).unwrap();
        for b in bars {
            assert!(f.bounds[0] <= b[0] && f.bounds[2] >= b[2] && f.bounds[1] <= b[1] && f.bounds[3] >= b[3], "{f:?}");
        }
        assert!(f.outline.iter().all(|&q| inside_ellipse(q, (e.0, e.1, e.2 - 1.0, e.3 - 1.0))), "{:?}", f.outline);
        assert_eq!(f.glyph, Some(18.0));
        let t = f.text;
        assert!(t[0] > f.bounds[0] && t[2] < f.bounds[2] && t[1] > f.bounds[1] && t[3] < f.bounds[3]);
    }

    /// A line lettered up to the balloon's edge, its strokes' soft edge
    /// lighter than ink: the cover takes in that edge too, where the paper
    /// has room (ch258 p4, a gray sliver beside the "I" of "NUREMI").
    #[test]
    fn a_strokes_soft_edge_is_covered_too() {
        let e = (200.0, 150.0, 120.0, 80.0);
        let bars = [[160, 110, 240, 128], [120, 141, 316, 159], [160, 172, 240, 190]];
        let mut px = page(400, 300, &[e], &bars);
        for y in 141..159 {
            px[y * 400 + 316] = 205;
        }
        let p = Page { w: 400, h: 300, px: &px };
        let f = find(&p, [120.0, 110.0, 316.0, 190.0], 18.0, &[]).unwrap();
        // Even-odd across the staircase's upright edges, right of the pixel.
        let covered = |x: i64, y: i64| {
            let o = &f.outline;
            (0..o.len()).filter(|&k| {
                let (a, b) = (o[k], o[(k + 1) % o.len()]);
                a[0] == b[0] && a[0] > x && y >= a[1].min(b[1]) && y < a[1].max(b[1])
            }).count() % 2 == 1
        };
        assert!((141..159).all(|y| covered(315, y) && covered(316, y)), "{:?}", f.outline);
    }

    /// Two balloons side by side: each cover keeps to its own.
    #[test]
    fn a_neighbouring_balloon_is_left_alone() {
        let (l, r) = ((100.0, 100.0, 80.0, 60.0), (290.0, 100.0, 80.0, 60.0));
        let px = page(400, 200, &[l, r], &[[70, 85, 130, 97], [70, 103, 130, 115], [260, 85, 320, 97], [260, 103, 320, 115]]);
        let p = Page { w: 400, h: 200, px: &px };
        // Gemini took both balloons for one.
        let f = find(&p, [70.0, 85.0, 130.0, 115.0], 12.0, &[[260.0, 85.0, 320.0, 115.0]]).unwrap();
        assert!(f.outline.iter().all(|&q| inside_ellipse(q, l)), "{:?}", f.outline);
    }

    /// A tight balloon on a screentone (ch256 p20: the dots outside the
    /// outline joined the lettering dot by dot until the whole panel was one
    /// block, and the panel went white). The block keeps to its letters.
    #[test]
    fn screentone_around_a_balloon_stays_out_of_its_lettering() {
        let (w, h) = (400u32, 300u32);
        let e = (150.0, 150.0, 58.0, 45.0);
        let mut px = page(w, h, &[e], &[]);
        for y in 0..h as i64 {
            for x in 0..w as i64 {
                let d = ((x as f64 - e.0) / e.2).powi(2) + ((y as f64 - e.1) / e.3).powi(2);
                let i = (y as u32 * w + x as u32) as usize;
                if d > 1.0 && px[i] != 0 {
                    // Paper with 2 px dots every 5 px, clear of the outline.
                    px[i] = if d > (1.0 + 5.0 / e.3).powi(2) && x % 5 < 2 && y % 5 < 2 { 0 } else { 255 };
                }
            }
        }
        // Two lines of hollow letters, 10 x 14.
        for (row, n) in [(132, 7), (150, 7)] {
            for k in 0..n {
                let (x0, y0) = (101 + k * 14, row);
                for y in y0..y0 + 14 {
                    for x in x0..x0 + 10 {
                        if x < x0 + 2 || x >= x0 + 8 || y < y0 + 2 || y >= y0 + 12 {
                            px[(y as u32 * w + x as u32) as usize] = 0;
                        }
                    }
                }
            }
        }
        let p = Page { w, h, px: &px };
        let b = blocks(&p, false);
        let text = b.iter().find(|b| b.letters >= 14).expect("the lettering is a block");
        let a = ABSORB * text.glyph + 1.0;
        assert!(text.bounds[0] >= 101.0 - a && text.bounds[2] <= 199.0 + a && text.bounds[1] >= 132.0 - a && text.bounds[3] <= 164.0 + a, "{:?}", text.bounds);
        assert!(text.in_balloon);
    }

    /// Gemini's box lands on the art next to its balloon (ch256 p20,
    /// "...YOU ARE?!"): the lettering in a balloon a little further off wins
    /// over the marks on the art the box grazes. A shout lettered on the art
    /// right under the box (ch256 p10, "AH!") keeps it over the whites of a
    /// pair of eyes further down. Boxes and blocks are from those pages.
    #[test]
    fn dialogue_goes_to_its_lettering_over_marks_on_the_art() {
        let block = |bounds: [f64; 4], letters, glyph, in_balloon| Block { bounds, letters, glyph, in_balloon };
        let hand = block([404.0, 450.0, 445.0, 482.0], 4, 16.0, false);
        let you_are = block([588.0, 384.0, 673.0, 438.0], 8, 24.0, true);
        assert_eq!(assign(&[([327.0, 410.0, 423.0, 480.0], 6)], &[hand, you_are], 1200), vec![Some(1)]);
        let ah = block([56.0, 750.0, 294.0, 816.0], 4, 55.0, false);
        let eyes = block([213.0, 1015.0, 270.0, 1099.0], 3, 38.0, true);
        assert_eq!(assign(&[([76.0, 767.0, 186.0, 818.0], 2)], &[ah, eyes], 1200), vec![Some(0)]);
        // ch256 p7: a shout lettered well inside a box drawn large, against
        // two marks in an eye's white a third of the page away.
        let wh = block([258.0, 1350.0, 379.0, 1388.0], 4, 34.0, false);
        let eye = block([706.0, 1393.0, 715.0, 1428.0], 2, 16.0, true);
        assert_eq!(assign(&[([266.0, 1326.0, 451.0, 1431.0], 2)], &[wh, eye], 1080), vec![Some(0)]);
    }

    /// ch257 p4: Gemini's boxes for "NOBODY INVITED YOU." and "OH,
    /// RENTAROU-KUN..." each sit a balloon to the right; nearest first gave
    /// each speaker the other's balloon.
    #[test]
    fn boxes_shifted_a_balloon_over_keep_their_order() {
        let block = |bounds: [f64; 4]| Block { bounds, letters: 14, glyph: 25.0, in_balloon: true };
        let blocks = [block([267.0, 415.0, 396.0, 530.0]), block([439.0, 432.0, 537.0, 537.0])];
        let items = [([403.0, 304.0, 567.0, 558.0], 16), ([595.0, 363.0, 735.0, 505.0], 13)];
        assert_eq!(assign(&items, &blocks, 1200), vec![Some(0), Some(1)]);
    }

    /// ch256 p17: "WELL" grouped apart from the rest of "WELL, UMM, I GUESS
    /// THERE'S IKU..." brought the caption beside it nearer its letter count;
    /// gathered there, it pushed this balloon's cover down to a sliver.
    #[test]
    fn a_caption_does_not_gather_letters_from_inside_another_items_lettering() {
        let block = |bounds: [f64; 4], letters, glyph| Block { bounds, letters, glyph, in_balloon: false };
        let blocks = [
            block([61.0, 823.0, 284.0, 1050.0], 42, 32.0),
            block([315.0, 782.0, 435.0, 947.0], 14, 29.0),
            block([375.0, 782.0, 415.0, 815.0], 3, 29.0),
        ];
        let items = [([33.0, 787.0, 247.0, 1047.0], 48), ([307.0, 725.0, 463.0, 989.0], 22)];
        let texts = gather(&items, &blocks, &[Some(0), Some(1)]);
        assert_eq!(texts, vec![vec![blocks[0].bounds], vec![blocks[1].bounds]]);
    }

    /// ch257 p8: "AGAIN, DAMN IT!..." short of its letter count took the
    /// sound effect below it, then the shout in the next panel, and its
    /// white box covered both panels.
    #[test]
    fn gathering_stays_on_the_items_box() {
        let block = |bounds: [f64; 4], letters, glyph| Block { bounds, letters, glyph, in_balloon: false };
        let blocks = [
            block([18.0, 839.0, 224.0, 1147.0], 26, 37.0),
            block([306.0, 1213.0, 467.0, 1638.0], 10, 42.0),
            block([511.0, 831.0, 609.0, 877.0], 5, 39.0),
        ];
        let texts = gather(&[([12.0, 834.0, 266.0, 1160.0], 39)], &blocks, &[Some(0)]);
        assert_eq!(texts, vec![vec![blocks[0].bounds]]);
    }

    /// ch256 p4: each of "7!" and "34!" lettered with its reading small
    /// over it ("nana", "san yon"). "34!" keeps the block in its own box,
    /// though that block has more letters than the item, and "7!" takes its
    /// reading in with it, though lettered smaller. A mark on the art
    /// touching a line is not joined so (ch257 p8, by "OH, I KNOW!"; its
    /// three marks counted a word here, so only its place on the art keeps
    /// it out). Boxes and blocks are from those pages.
    #[test]
    fn a_reading_lettered_over_its_line_goes_with_it() {
        let block = |bounds: [f64; 4], letters, glyph| Block { bounds, letters, glyph, in_balloon: true };
        let blocks = [
            block([684.0, 1539.0, 781.0, 1673.0], 18, 23.0),
            block([316.0, 1563.0, 354.0, 1573.0], 4, 10.0),
            block([316.0, 1578.0, 354.0, 1613.0], 3, 33.0),
            block([467.0, 1585.0, 535.0, 1633.0], 9, 10.0),
        ];
        let items = [([300.0, 1466.9, 420.0, 1611.3], 1), ([465.6, 1466.9, 585.6, 1611.3], 2), ([630.0, 1466.9, 750.0, 1611.3], 19)];
        let on = assign(&items, &blocks, 1200);
        assert_eq!(on, vec![Some(2), Some(3), Some(0)]);
        assert_eq!(gather(&items, &blocks, &on)[0], vec![blocks[2].bounds, blocks[1].bounds]);

        let own = Block { bounds: [796.0, 937.0, 967.0, 1157.0], letters: 38, glyph: 21.0, in_balloon: false };
        let face = Block { bounds: [889.0, 899.0, 906.0, 926.0], letters: 4, glyph: 11.0, in_balloon: false };
        assert_eq!(gather(&[([858.0, 834.6, 1050.0, 1160.7], 64)], &[own.clone(), face], &[Some(0)]), vec![vec![own.bounds]]);
    }

    /// ch257 p10: the footnote's box sits 24 px above its line, which the
    /// grouping split in three; the box moves onto the line and gathers all
    /// of it, not the 42 px lettering further off.
    #[test]
    fn a_box_off_its_line_moves_onto_it() {
        let block = |bounds: [f64; 4], letters, glyph| Block { bounds, letters, glyph, in_balloon: false };
        let blocks = [
            block([268.0, 1016.0, 447.0, 1034.0], 14, 13.0),
            block([477.0, 1016.0, 743.0, 1039.0], 23, 13.0),
            block([744.0, 1016.0, 860.0, 1039.0], 10, 13.0),
            block([384.0, 1111.0, 501.0, 1197.0], 6, 42.0),
        ];
        let mut items = [([379.0, 932.0, 1146.0, 992.0], 52)];
        let mut on = assign(&items, &blocks, 1200);
        assert_eq!(on, vec![None]);
        nudge(&mut items, &blocks, &mut on);
        assert_eq!(on, vec![Some(1)]);
        let mut texts = gather(&items, &blocks, &on).remove(0);
        texts.sort_by(|a, b| a[0].total_cmp(&b[0]));
        assert_eq!(texts, vec![blocks[0].bounds, blocks[1].bounds, blocks[2].bounds]);
    }

    /// Lettering on a face (ch256 p8, "...SHE NEEDS OPPORTUNITIES"): the
    /// white skin round it looked like a balloon, and its cover wiped the
    /// closed eye below the text. Paper holding art away from the lettering
    /// is the picture.
    #[test]
    fn lettering_on_white_skin_is_not_a_balloon() {
        let (w, h) = (400u32, 300u32);
        let mut px = vec![90u8; (w * h) as usize];
        for y in 50..250 {
            for x in 50..350 {
                px[(y * w + x) as usize] = 255;
            }
        }
        for b in [[120, 80, 280, 98], [120, 105, 280, 123], [150, 190, 250, 196]] {
            for y in b[1]..b[3] {
                for x in b[0]..b[2] {
                    px[(y as u32 * w + x as u32) as usize] = 0;
                }
            }
        }
        let p = Page { w, h, px: &px };
        assert_eq!(find(&p, [120.0, 80.0, 280.0, 123.0], 18.0, &[]), None);
    }

    /// Text over the art, with no balloon around it, is not taken for one.
    #[test]
    fn text_without_a_closed_balloon_is_not_found() {
        let px = vec![255u8; 300 * 200];
        let p = Page { w: 300, h: 200, px: &px };
        assert_eq!(find(&p, [100.0, 80.0, 200.0, 120.0], 20.0, &[]), None);
    }
}
