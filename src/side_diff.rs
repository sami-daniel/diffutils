// This file is part of the uutils diffutils package.
//
// For the full copyright and license information, please view the LICENSE-*
// files that was distributed with this source code.

use core::cmp::{max, min};
use diff::Result;
use std::{
    io::{stdout, StdoutLock, Write},
    vec,
};
use unicode_width::UnicodeWidthChar;

const GUTTER_WIDTH_MIN: isize = 3; // The MIDDLE mark size
const C_WIDTH: isize = 130; // Can be overrided by -w option, just an temporary solution

struct Modifiers {
    expanded: bool,
    tab_size: usize,
    sdiff_half_width: usize,
}

impl Modifiers {
    pub fn new(sdiff_half_width: usize, tab_size: usize, expanded: bool) -> Modifiers {
        Modifiers {
            expanded,
            tab_size,
            sdiff_half_width,
        }
    }
}

fn format_tabs_and_spaces(
    from: usize,
    to: usize,
    tab_size: usize,
    expanded: bool,
    buf: &mut Vec<u8>,
) {
    let mut current = from;

    if current > to {
        return;
    }

    if expanded {
        buf.extend(vec![b' '; to - current]);
        return;
    }

    while current + (tab_size - current % tab_size) <= to {
        let next_tab = current + (tab_size - current % tab_size);
        buf.push(b'\t');
        current = next_tab;
    }

    let remaining_spaces = to - current;
    buf.extend(vec![b' '; remaining_spaces]);
}

fn process_half_line(
    s: &[u8],
    max_width: usize,
    expanded: bool,
    tab_size: usize,
    is_right: bool,
    buf: &mut Vec<u8>,
) -> std::io::Result<()> {
    let mut current_width = 0;
    let mut is_utf8 = false;
    let iter = s.iter();
    let input = match String::from_utf8(s.to_vec()) { // third buffer
        Ok(s) => {
            is_utf8 = true;
            s
        }
        Err(_) => String::new(),
    };

    if is_right && !s.is_empty() {
        buf.push(b' ');
    }

    // The encoding will probably be compatible with utf8, so we can take advantage
    // of that to get the size of the columns and iterate without breaking the encoding of anything.
    // It seems like a good trade, since there is still a fallback in case it is not utf8.
    if is_utf8 {
        let chars = input.chars();

        for c in chars {
            let c_width = UnicodeWidthChar::width(c).unwrap_or(1);
            if current_width + c_width > max_width {
                break; // it will never cut a multibyte char
            }

            match c {
                '\t' => {
                    if expanded {
                        let spaces = tab_size - (current_width % tab_size);
                        buf.extend(vec![b' '; spaces]);
                        current_width += spaces;
                    } else {
                        buf.push(b'\t');
                        current_width += tab_size - (current_width % tab_size);
                    }
                }
                '\n' => {
                    if is_right {
                        buf.push(b'\n');
                    }
                    break;
                }
                '\r' => {
                    continue;
                }
                _ => {
                    buf.write_all(c.to_string().as_bytes())?;
                    current_width += c_width;
                }
            }
        }
    } else {
        for c in iter {
            if current_width + 1 > max_width {
                break; // maybe can cut the character if it is multibyte
            }

            match *c {
                b'\t' => {
                    if expanded {
                        let spaces = tab_size - (current_width % tab_size);
                        buf.extend(vec![b' '; spaces]);
                        current_width += spaces;
                    } else {
                        buf.push(b'\t');
                        current_width += tab_size - (current_width % tab_size);
                    }
                }
                b'\n' => {
                    break;
                }
                b'\r' => {
                    continue;
                }
                _ => {
                    buf.push(*c);
                    current_width += 1;
                }
            }
        }
    }

    // gnu sdiff do not tabulates the hole empty right line, instead, just keep the line empty
    if !is_right || !s.is_empty() {
        format_tabs_and_spaces(
            current_width,
            max_width + if !is_right { 1 } else { 0 },
            tab_size,
            expanded,
            buf,
        );
    }

    Ok(())
}

fn push_output(
    output: &mut StdoutLock,
    left_ln: &[u8],
    right_ln: &[u8],
    symbol: u8,
    left_ln_buffer: &mut Vec<u8>,
    right_ln_buffer: &mut Vec<u8>,
    modifiers: &Modifiers,
) -> std::io::Result<()> {
    left_ln_buffer.clear();
    right_ln_buffer.clear();

    process_half_line(
        left_ln,
        modifiers.sdiff_half_width,
        modifiers.expanded,
        modifiers.tab_size,
        false,
        left_ln_buffer,
    )
    .unwrap();
    process_half_line(
        right_ln,
        modifiers.sdiff_half_width,
        modifiers.expanded,
        modifiers.tab_size,
        true,
        right_ln_buffer,
    )
    .unwrap();

    output.write_all(left_ln_buffer)?;
    output.write_all(&[symbol])?;
    output.write_all(right_ln_buffer)?;

    // gnu side diff only prints the \n on right line if the line contains the char
    writeln!(output)?;

    Ok(())
}

pub fn diff(from_file: &[u8], to_file: &[u8]) -> Vec<u8> {
    //      ^ The left file  ^ The right file

    let mut output = stdout().lock();
    let left_lines: Vec<&[u8]> = from_file.split(|&c| c == b'\n').collect();
    let right_lines: Vec<&[u8]> = to_file.split(|&c| c == b'\n').collect();

    // for some reason that I could not identify, GNU diff
    // use this calc to calculate the size of  half line
    // based on options passed (like -w, -t, etc. ). Actually, is pretty uselles, cause we
    // dont have any size modifiers that can alter this, however
    // I just want to leave here the calc, since it's not clear
    // and can make some sort of mess

    let t = /* expanded_tabs ? 1 : tabsize, however, we dont have -t opt, so it will be always 8 */ 8;
    let t_plus_g = t + GUTTER_WIDTH_MIN;
    let unaligned_off = (C_WIDTH >> 1) + (t_plus_g >> 1) + (C_WIDTH & t_plus_g & 1);
    let off = unaligned_off - unaligned_off % t;
    let sdiff_half_width = (max(0, min(off - GUTTER_WIDTH_MIN, C_WIDTH - off))) as usize;

    let mut left_ln_buf = Vec::with_capacity(sdiff_half_width);
    let mut right_ln_buf = Vec::with_capacity(sdiff_half_width);

    let modifiers = Modifiers::new(sdiff_half_width, t as usize, false);

    for result in diff::slice(&left_lines, &right_lines) {
        match result {
            Result::Left(left_ln) => {
                push_output(
                    &mut output,
                    left_ln,
                    b"",
                    b'<',
                    &mut left_ln_buf,
                    &mut right_ln_buf,
                    &modifiers,
                )
                .unwrap();
            }
            Result::Right(right_ln) => {
                push_output(
                    &mut output,
                    b"",
                    right_ln,
                    b'>',
                    &mut left_ln_buf,
                    &mut right_ln_buf,
                    &modifiers,
                )
                .unwrap();
            }
            Result::Both(left_ln, right_ln) => {
                push_output(
                    &mut output,
                    left_ln,
                    right_ln,
                    b' ',
                    &mut left_ln_buf,
                    &mut right_ln_buf,
                    &modifiers,
                )
                .unwrap();
            }
        }
    }

    vec![]
}

#[cfg(test)]
mod tests {
    const DEF_TABSIZE: usize = 4;

    use super::*;

    #[test]
    fn test_format_tabs_and_spaces_expanded_false() {
        let mut buf = Vec::new();
        format_tabs_and_spaces(0, 5, DEF_TABSIZE, false, &mut buf);
        assert_eq!(buf, vec![b'\t', b' ']);
    }

    #[test]
    fn test_format_tabs_and_spaces_expanded_true() {
        let mut buf = Vec::new();
        format_tabs_and_spaces(0, 5, DEF_TABSIZE, true, &mut buf);
        assert_eq!(buf, vec![b' '; 5]);
    }

    #[test]
    fn test_format_tabs_and_spaces_from_greater_than_to() {
        let mut buf = Vec::new();
        format_tabs_and_spaces(6, 3, DEF_TABSIZE, false, &mut buf);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_format_from_non_zero_position() {
        let mut buf = Vec::new();
        format_tabs_and_spaces(2, 7, DEF_TABSIZE, false, &mut buf);
        assert_eq!(buf, vec![b'\t', b' ', b' ', b' ']);
    }

    #[test]
    fn test_multiple_full_tabs_needed() {
        let mut buf = Vec::new();
        format_tabs_and_spaces(0, 12, DEF_TABSIZE, false, &mut buf);
        assert_eq!(buf, vec![b'\t', b'\t', b'\t']);
    }

    #[test]
    fn test_uneven_tab_boundary_with_spaces() {
        let mut buf = Vec::new();
        format_tabs_and_spaces(3, 10, DEF_TABSIZE, false, &mut buf);
        assert_eq!(buf, vec![b'\t', b'\t', b' ', b' ']);
    }

    #[test]
    fn test_expanded_true_with_offset() {
        let mut buf = Vec::new();
        format_tabs_and_spaces(3, 9, DEF_TABSIZE, true, &mut buf);
        assert_eq!(buf, vec![b' '; 6]);
    }

    #[test]
    fn test_exact_tab_boundary_from_midpoint() {
        let mut buf = Vec::new();
        format_tabs_and_spaces(4, 8, DEF_TABSIZE, false, &mut buf);
        assert_eq!(buf, vec![b'\t']);
    }

    #[test]
    fn test_mixed_tabs_and_spaces_edge_case() {
        let mut buf = Vec::new();
        format_tabs_and_spaces(5, 9, DEF_TABSIZE, false, &mut buf);
        assert_eq!(buf, vec![b'\t', b' ']);
    }

    #[test]
    fn test_minimal_gap_with_tab() {
        let mut buf = Vec::new();
        format_tabs_and_spaces(7, 8, DEF_TABSIZE, false, &mut buf);
        assert_eq!(buf, vec![b'\t']);
    }

    #[test]
    fn test_expanded_false_with_tab_at_end() {
        let mut buf = Vec::new();
        format_tabs_and_spaces(6, 8, DEF_TABSIZE, false, &mut buf);
        assert_eq!(buf, vec![b'\t']);
    }
}