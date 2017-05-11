// Copyright 2014 The html5ever Project Developers. See the
// COPYRIGHT file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! 3.3.4 - 3.3.6. Resolve implicit levels and types.

use super::{BidiClass, class_for_level, is_rtl, LevelRun};
use super::BidiClass::*;
use super::prepare::{IsolatingRunSequence, not_removed_by_x9, removed_by_x9};
use std::cmp::max;

/// 3.3.4 Resolving Weak Types
///
/// http://www.unicode.org/reports/tr9/#Resolving_Weak_Types
pub fn resolve_weak(sequence: &IsolatingRunSequence, classes: &mut [BidiClass]) {
    // FIXME (#8): This function applies steps W1-W6 in a single pass.  This can produce
    // incorrect results in cases where a "later" rule changes the value of `prev_class` seen
    // by an "earlier" rule.  We should either split this into separate passes, or preserve
    // extra state so each rule can see the correct previous class.

    let mut prev_class = sequence.sos;
    let mut last_strong_is_al = false;
    let mut et_run_indices = Vec::new(); // for W5

    // Like sequence.runs.iter().flat_map(Clone::clone), but make indices itself clonable.
    fn id(x: LevelRun) -> LevelRun { x }
    let mut indices = sequence.runs.iter().cloned().flat_map(id as fn(LevelRun) -> LevelRun);

    while let Some(i) = indices.next() {
        match classes[i] {
            // http://www.unicode.org/reports/tr9/#W1
            NSM => {
                classes[i] = match prev_class {
                    RLI | LRI | FSI | PDI => ON,
                    _ => prev_class
                };
            }
            EN => {
                if last_strong_is_al {
                    // W2. If previous strong char was AL, change EN to AN.
                    classes[i] = AN;
                } else {
                    // W5. If a run of ETs is adjacent to an EN, change the ETs to EN.
                    for j in &et_run_indices {
                        classes[*j] = EN;
                    }
                    et_run_indices.clear();
                }
            }
            // http://www.unicode.org/reports/tr9/#W3
            AL => classes[i] = R,

            // http://www.unicode.org/reports/tr9/#W4
            ES | CS => {
                let next_class = indices.clone().map(|j| classes[j]).filter(not_removed_by_x9)
                    .next().unwrap_or(sequence.eos);
                classes[i] = match (prev_class, classes[i], next_class) {
                    (EN, ES, EN) |
                    (EN, CS, EN) => EN,
                    (AN, CS, AN) => AN,
                    (_,  _,  _ ) => ON,
                }
            }
            // http://www.unicode.org/reports/tr9/#W5
            ET => {
                match prev_class {
                    EN => classes[i] = EN,
                    _ => et_run_indices.push(i) // In case this is followed by an EN.
                }
            }
            class => if removed_by_x9(class) {
                continue
            }
        }

        prev_class = classes[i];
        match prev_class {
            L | R => { last_strong_is_al = false; }
            AL => { last_strong_is_al = true;  }
            _ => {}
        }
        if prev_class != ET {
            // W6. If we didn't find an adjacent EN, turn any ETs into ON instead.
            for j in &et_run_indices {
                classes[*j] = ON;
            }
            et_run_indices.clear();
        }
    }

    // W7. If the previous strong char was L, change EN to L.
    let mut last_strong_is_l = sequence.sos == L;
    for run in &sequence.runs {
        for i in run.clone() {
            match classes[i] {
                EN if last_strong_is_l => { classes[i] = L; }
                L => { last_strong_is_l = true; }
                R | AL => { last_strong_is_l = false; }
                _ => {}
            }
        }
    }
}

/// 3.3.5 Resolving Neutral Types
///
/// http://www.unicode.org/reports/tr9/#Resolving_Neutral_Types
pub fn resolve_neutral(sequence: &IsolatingRunSequence, levels: &[u8],
                       classes: &mut [BidiClass])
{
    let mut indices = sequence.runs.iter().flat_map(Clone::clone);
    let mut prev_class = sequence.sos;

    // Neutral or Isolate formatting characters (NI).
    // http://www.unicode.org/reports/tr9/#NI
    fn ni(class: BidiClass) -> bool {
        matches!(class, B | S | WS | ON | FSI | LRI | RLI | PDI)
    }

    while let Some(mut i) = indices.next() {
        // N0. Process bracket pairs.
        // TODO

        // Process sequences of NI characters.
        let mut ni_run = Vec::new();
        if ni(classes[i]) {
            // Consume a run of consecutive NI characters.
            ni_run.push(i);
            let mut next_class;
            loop {
                match indices.next() {
                    Some(j) => {
                        i = j;
                        if removed_by_x9(classes[i]) {
                            continue
                        }
                        next_class = classes[j];
                        if ni(next_class) {
                            ni_run.push(i);
                        } else {
                            break
                        }
                    }
                    None => {
                        next_class = sequence.eos;
                        break
                    }
                };
            }

            // N1-N2.
            let new_class = match (prev_class, next_class) {
                (L,  L ) => L,
                (R,  R ) |
                (R,  AN) |
                (R,  EN) |
                (AN, R ) |
                (AN, AN) |
                (AN, EN) |
                (EN, R ) |
                (EN, AN) |
                (EN, EN) => R,
                (_,  _ ) => class_for_level(levels[i]),
            };
            for j in &ni_run {
                classes[*j] = new_class;
            }
            ni_run.clear();
        }
        prev_class = classes[i];
    }
}

/// 3.3.6 Resolving Implicit Levels
///
/// Returns the maximum embedding level in the paragraph.
///
/// http://www.unicode.org/reports/tr9/#Resolving_Implicit_Levels
pub fn resolve_levels(classes: &[BidiClass], levels: &mut [u8]) -> u8 {
    let mut max_level = 0;

    assert!(classes.len() == levels.len());
    for i in 0..levels.len() {
        match (is_rtl(levels[i]), classes[i]) {
            // http://www.unicode.org/reports/tr9/#I1
            (false, R)  => levels[i] += 1,
            (false, AN) |
            (false, EN) => levels[i] += 2,
            // http://www.unicode.org/reports/tr9/#I2
            (true, L)  |
            (true, EN) |
            (true, AN) => levels[i] += 1,
            (_, _) => {}
        }
        max_level = max(max_level, levels[i]);
    }
    max_level
}