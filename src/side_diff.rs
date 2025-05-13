// This file is part of the uutils diffutils package.
//
// For the full copyright and license information, please view the LICENSE-*
// files that was distributed with this source code.

use diff::Result;
use std::{
    io::{stdout, StdoutLock, Write},
    vec,
};
use unicode_width::UnicodeWidthChar;

const SDIFF_HALF_WIDTH: usize = 60;
const TAB_SIZE: usize = 8;

fn format_tabs_and_spaces(from: usize, to: usize, tab_size: usize, expanded: bool) -> Vec<u8> {
    let mut output = vec![]; // Fourty buffer created
    let mut current = from;

    if !expanded {
        while current + (tab_size - current % tab_size) <= to {
            let next_tab = current + (tab_size - current % tab_size);
            output.push(b'\t');
            current = next_tab;
        }
    } else {
        output.extend(vec![b' '; current + (tab_size - current % tab_size)]);
        current = current + (tab_size - current % tab_size);
    }

    let remaining_spaces = to - current;
    output.extend(vec![b' '; remaining_spaces]);

    output
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
    let input = match String::from_utf8(s.to_vec()) { // Third buffer created
        Ok(s) => {
            is_utf8 = true;
            s
        }
        Err(_) => String::new(),
    };

    if is_right && !s.is_empty() {
        buf.push(b' ');
    }

    //The encoding will probably be compatible with utf8, so we can take advantage
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
        let padding = format_tabs_and_spaces(
            current_width,
            max_width + if !is_right { 1 } else { 0 },
            tab_size,
            expanded,
        );
        buf.extend(padding);
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
) -> std::io::Result<()> {
    const EXPANDED: bool = false; // should come from the flag -t,

    left_ln_buffer.clear();
    right_ln_buffer.clear();

    process_half_line(
        left_ln,
        SDIFF_HALF_WIDTH + 1,
        EXPANDED,
        TAB_SIZE,
        false,
        left_ln_buffer,
    )
    .unwrap();
    process_half_line(
        right_ln,
        SDIFF_HALF_WIDTH + 1,
        EXPANDED,
        TAB_SIZE,
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

    let mut left_ln_buf = Vec::with_capacity(SDIFF_HALF_WIDTH + 1);
    let mut right_ln_buf = Vec::with_capacity(SDIFF_HALF_WIDTH + 1);

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
                )
                .unwrap();
            }
        }
    }

    vec![]
}
