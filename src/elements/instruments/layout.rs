//! Where the widgets go: groups at anchors, in rows or columns, sized in cards.

use super::draw::{Area, card_scale, card_size};
use crate::config::{Anchor, Direction, Group};
use crate::scene::{Edges, Geo, Rect};
use crate::title::bracket_corners;
use crate::tweaks::TitleCorner;
use crate::util::unit;
use std::collections::HashMap;

fn overlaps(a: Rect, b: Rect) -> bool {
    a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3
}

fn anchor_name(a: Anchor) -> &'static str {
    match a {
        Anchor::UnderTitle => "under-title",
        Anchor::TopLeft => "top-left",
        Anchor::TopRight => "top-right",
        Anchor::BottomLeft => "bottom-left",
        Anchor::BottomRight => "bottom-right",
        Anchor::Status => "status",
    }
}

/// Each widget's area by name, on whole pixels. Groups too wide are scaled down; groups that would
/// overlap the hostname or an earlier group are moved clear. The status line places itself.
pub fn layout(g: &Geo, groups: &[Group], sizes: &HashMap<String, (i32, i32)>) -> HashMap<String, Area> {
    let th = &g.th;
    let (width, height) = (g.w, g.h);
    let u = unit(width, height);
    let mut areas = HashMap::new();
    let mut placed: Vec<Rect> = vec![th.title_box(width, height, &g.host, g.edges)];
    let mut order: Vec<&Group> = groups.iter().collect();
    order.sort_by_key(|gr| gr.anchor != Anchor::UnderTitle);
    for group in order {
        let (anchor, column) = (group.anchor, group.direction == Direction::Column);
        let under_title = anchor == Anchor::UnderTitle;
        let named: Vec<(&String, (i32, i32))> = group.widgets.iter().filter_map(|n| sizes.get(n).map(|&s| (n, s))).collect();
        if anchor == Anchor::Status || named.is_empty() {
            continue;
        }
        let inset = (th.margin(width, height)).round();
        let own = if group.edges { g.edges } else { Edges::NONE };
        let Edges { top: top_, right: right_, left: left_, .. } = own;
        let mut scale = card_scale(th, width, height);
        let (cw, ch, gap) = card_size(th, width, height);
        let spans = |cw: i32, ch: i32, gap: i32| -> (Vec<(i32, i32)>, i32, i32) {
            let dims: Vec<(i32, i32)> = named.iter().map(|&(_, (c, r))| (c * cw + (c - 1) * gap, r * ch + (r - 1) * gap)).collect();
            let (ws, hs) = (dims.iter().map(|d| d.0), dims.iter().map(|d| d.1));
            let between = gap * (dims.len() as i32 - 1);
            if column {
                let (w, h) = (ws.max().unwrap_or(0), hs.sum::<i32>() + between);
                (dims, w, h)
            } else {
                let (w, h) = (ws.sum::<i32>() + between, hs.max().unwrap_or(0));
                (dims, w, h)
            }
        };
        let (mut dims, mut span_w, mut span_h) = spans(cw, ch, gap);
        let mut gap = gap;
        let room = width as f64 - 2.0 * inset - if !under_title { (left_ + right_) as f64 } else { 0.0 };
        if span_w as f64 > room {
            // too wide: scale down
            scale *= room / span_w as f64;
            let (cw, ch) = ((300.0 * scale).round() as i32, (th.tw.card_h * scale).round() as i32);
            gap = (th.tw.instrument_gap * scale).round() as i32;
            (dims, span_w, span_h) = spans(cw, ch, gap);
        }
        let (span_w, span_h, gapf) = (span_w as f64, span_h as f64, gap as f64);
        let (left, top, right);
        if under_title {
            right = (th.title_right(width, height)).round();
            top = (th.instruments_top(width, height, &g.host)).round();
            left = right - span_w;
        } else {
            right = 0.0;
            let upper = matches!(anchor, Anchor::TopLeft | Anchor::TopRight);
            left = if matches!(anchor, Anchor::TopLeft | Anchor::BottomLeft) { left_ as f64 + inset } else { (width - right_) as f64 - inset - span_w };
            let b = bracket_corners(width, height, own);
            let mut t = if upper { (b.top + 0.065 * u).round() } else { (b.bottom - 0.065 * u - span_h).round() };
            if th.tw.corner != 0.0 && upper && th.tw.title_corner == TitleCorner::TopRight {
                t = (th.title_place(width, height, &g.host, Edges::NONE).y0).round(); // level with the lettering's top
            } else if th.tw.corner != 0.0 && upper {
                t = top_ as f64 + inset;
            }
            for _ in 0..placed.len() + 1 {
                // clear of the hostname and the groups before it
                let me = (left, t, left + span_w, t + span_h);
                let hit: Vec<Rect> = placed.iter().cloned().filter(|&r| overlaps(r, me)).collect();
                if hit.is_empty() {
                    break;
                }
                t = if upper {
                    hit.iter().map(|r| r.3).fold(f64::MIN, f64::max) + gapf
                } else {
                    hit.iter().map(|r| r.1).fold(f64::MAX, f64::min) - gapf - span_h
                };
            }
            if t < 0.0 || t + span_h > height as f64 {
                crate::errln!("layout: the {} group does not fit on a {width}x{height} screen", anchor_name(anchor));
            }
            top = t;
        }
        placed.push((left, top, left + span_w, top + span_h));
        let (mut x, mut y) = (left, top);
        for (&(n, _), &(dw, dh)) in named.iter().zip(dims.iter()) {
            let (dw, dh) = (dw as f64, dh as f64);
            if column {
                areas.insert(
                    n.to_string(),
                    Area { x: if matches!(anchor, Anchor::TopRight | Anchor::BottomRight) { left + span_w - dw } else { left }, y, w: dw, h: dh, scale },
                );
                y += dh + gapf;
            } else {
                areas.insert(n.to_string(), Area { x, y: top, w: dw, h: dh, scale });
                x += dw + gapf;
            }
        }
        if under_title && !column {
            // placed card by card from the right
            let mut x = right;
            for (&(n, _), &(dw, dh)) in named.iter().zip(dims.iter()).rev() {
                let (dw, dh) = (dw as f64, dh as f64);
                x -= dw;
                areas.insert(n.to_string(), Area { x, y: top, w: dw, h: dh, scale });
                x -= gapf;
            }
        }
    }
    areas
}
