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
const WIDTH: isize = 130; // Can be overrided by -w option, just an temporary solution

fn format_tabs_and_spaces(
    from: usize,
    to: usize,
    tab_size: usize,
    expanded: bool,
    buf: &mut Vec<u8>,
) {
    let mut current = from;

    if !expanded {
        while current + (tab_size - current % tab_size) <= to {
            let next_tab = current + (tab_size - current % tab_size);
            buf.push(b'\t');
            current = next_tab;
        }
    } else {
        buf.extend(vec![b' '; current + (tab_size - current % tab_size)]);
        current = current + (tab_size - current % tab_size);
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
    let input = match String::from_utf8(s.to_vec()) {
        // Third buffer created
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
    sdiff_half_width: usize,
    tab_size: usize,
) -> std::io::Result<()> {
    const EXPANDED: bool = false; // should come from the flag -t,

    left_ln_buffer.clear();
    right_ln_buffer.clear();

    process_half_line(
        left_ln,
        sdiff_half_width,
        EXPANDED,
        tab_size,
        false,
        left_ln_buffer,
    )
    .unwrap();
    process_half_line(
        right_ln,
        sdiff_half_width,
        EXPANDED,
        tab_size,
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
    // The first part gets the exactly half of WIDTH rounded floor,
    // then sum with the exactly half of t_plus_g rounded floor,
    // then, if both WIDTH and t_plus_g are odd, add one
    let unaligned_off = (WIDTH >> 1) + (t_plus_g >> 1) + (WIDTH & t_plus_g & 1);
    // unaligned_off - the next of t, rounded floor, cause the sdiff_half_width + GUTTER_WIDTH_MAX
    // has always to be an multiple of tabsize, to garantee the alignment
    let off = unaligned_off - unaligned_off % t;
    let sdiff_half_width = (max(0, min(off - GUTTER_WIDTH_MIN, WIDTH - off))) as usize;

    let mut left_ln_buf = Vec::with_capacity(sdiff_half_width);
    let mut right_ln_buf = Vec::with_capacity(sdiff_half_width);

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
                    sdiff_half_width,
                    t as usize,
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
                    sdiff_half_width,
                    t as usize,
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
                    sdiff_half_width,
                    t as usize,
                )
                .unwrap();
            }
        }
    }

    vec![]
}
