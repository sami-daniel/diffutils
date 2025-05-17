// This file is part of the uutils diffutils package.
//
// For the full copyright and license information, please view the LICENSE-*
// files that was distributed with this source code.

use core::cmp::{max, min};
use diff::Result;
use std::{
    io::{stdout, Write},
    vec,
};
use unicode_width::UnicodeWidthChar;

const GUTTER_WIDTH_MIN: usize = 3;
const FULL_WIDTH: usize = 130;
const TAB_SIZE: usize = 8;

struct Config {
    sdiff_half_width: usize,
    sdiff_column_two_offset: usize,
    tab_size: usize,
    expanded: bool,
    separator_pos: usize,
}

struct OutputHandler<'a, T>
where
    T: Write,
{
    output: &'a mut T,
    config: Config,
    left_ln_buf: Vec<u8>, // May we waisting a lot of memory doing this, instead of simply push to stdout
    right_ln_buf: Vec<u8>,
}

struct LineFormatter<'a> {
    config: &'a Config,
    buf: &'a mut Vec<u8>,
}

impl Config {
    pub fn new(full_width: usize, tab_size: usize, expanded: bool) -> Self {
        // diff uses this calculation to calculate the size of a half line
        // based on the options passed (like -w, -t, etc.). It's actually
        // pretty useless, because we (actually) don't have any size modifiers
        // that can change this, however I just want to leave the calculate
        // here, since it's not very clear and may cause some confusion

        let w = full_width as isize;
        let t = tab_size as isize;
        let t_plus_g = t + GUTTER_WIDTH_MIN as isize;
        let unaligned_off = (w >> 1) + (t_plus_g >> 1) + (w & t_plus_g & 1);
        let off = unaligned_off - unaligned_off % t;
        let hw = max(0, min(off - GUTTER_WIDTH_MIN as isize, w - off)) as usize;
        let c2o = if hw != 0 { off as usize } else { w as usize };

        Self {
            expanded,
            sdiff_column_two_offset: c2o,
            tab_size,
            sdiff_half_width: hw,
            separator_pos: ((hw + c2o - 1) >> 1),
        }
    }
}

impl<'a> LineFormatter<'a> {
    fn new(config: &'a Config, buf: &'a mut Vec<u8>) -> Self {
        Self { config, buf }
    }

    fn format_tabs_and_spaces(&mut self, from: usize, to: usize) {
        let expanded = self.config.expanded;
        let buf = &mut self.buf;
        let tab_size = self.config.tab_size;
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

        buf.extend(vec![b' '; to - current]);
    }

    fn process_half_line(
        &mut self,
        s: &[u8],
        max_width: usize,
        is_right: bool,
        white_space_gutter: bool,
    ) -> std::io::Result<()> {
        if max_width > self.config.sdiff_half_width {
            return Ok(());
        }
        
        if max_width > self.config.sdiff_column_two_offset && !is_right {
            return Ok(());
        }
        
        let expanded = self.config.expanded;
        let tab_size = self.config.tab_size;
        let sdiff_column_two_offset = self.config.sdiff_column_two_offset;
        let mut current_width = 0;
        let mut is_utf8 = false;
        let iter = s.iter();
        let input = match String::from_utf8(s.to_vec()) {
            // third buffer created
            Ok(s) => {
                is_utf8 = true;
                s
            }
            Err(_) => String::new(),
        };

        // the encoding will probably be compatible with utf8, so we can take advantage
        // of that to get the size of the columns and iterate without breaking the encoding of anything.
        // It seems like a good trade, since there is still a fallback in case it is not utf8.
        // But I think it would be better if we used some lib that would allow us to handle this
        // in the best way possible, in order to avoid overhead (currently 2 for loops are needed).
        // There is a library called mcel (mcel.h) that is used in GNU diff, but the documentation
        // about it is very scarce, nor is its use documented on the internet. In fact, from my
        // research I didn't even find any information about it in the GNU lib's own documentation.
        if is_utf8 {
            // avoiding the creation of chars variables
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
                            if current_width + spaces <= max_width {
                                self.buf.extend(vec![b' '; spaces]);
                                current_width += spaces;
                            }
                        } else {
                            if current_width + tab_size - (current_width % tab_size) <= max_width {
                                self.buf.push(b'\t');
                                current_width += tab_size - (current_width % tab_size);
                            }
                        }
                    }
                    '\n' => {
                        break;
                    }
                    '\r' => {
                        self.buf.push(b'\r');
                        self.format_tabs_and_spaces(0, sdiff_column_two_offset);
                        current_width = 0;
                    },
                    '\0' | '\x07' | '\x0C' | '\x0B' => {
                        self.buf.push(c as u8);
                    },
                    _ => {
                        self.buf.write_all(c.to_string().as_bytes())?;
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
                            if current_width + spaces <= max_width {
                                self.buf.extend(vec![b' '; spaces]);
                                current_width += spaces;
                            }
                        } else {
                            if current_width + tab_size - (current_width % tab_size) <= max_width {
                                self.buf.push(b'\t');
                                current_width += tab_size - (current_width % tab_size);
                            }
                        }
                    }
                    b'\n' => {
                        break;
                    }
                    b'\r' => {
                        self.buf.push(b'\r');
                        self.format_tabs_and_spaces(0, sdiff_column_two_offset);
                        current_width = 0;
                    },
                    b'\0' | b'\x07' | b'\x0C' | b'\x0B' => {
                        // width 0, just print it
                        self.buf.push(*c);
                    },
                    _ => {
                        self.buf.push(*c);
                        current_width += 1;
                    }
                }
            }
        }

        // gnu sdiff do not tabulate the hole empty right line, instead, just keep the line empty
        if !is_right {
            // we always sum + 1 or + GUTTER_WIDTH_MIN cause we want to expand
            // to the third column o the gutter with is gutter white space, otherwise
            // we can expand to only the first column of the gutter middle column, cause
            // the next is the sep char
            self.format_tabs_and_spaces(
                current_width,
                max_width
                    + if white_space_gutter {
                        GUTTER_WIDTH_MIN
                    } else {
                        1
                    },
            );
        }

        Ok(())
    }
}

impl<'a, T> OutputHandler<'a, T>
where
    T: Write,
{
    fn new(config: Config, output: &'a mut T) -> Self {
        let hw = config.sdiff_half_width;

        Self {
            config,
            output,
            // + 3 cause the left line may expand to GUTTER_WIDTH_MIN, so prealloc them
            left_ln_buf: Vec::with_capacity(hw + GUTTER_WIDTH_MIN),
            right_ln_buf: Vec::with_capacity(hw),
        }
    }

    fn push_output(&mut self, left_ln: &[u8], right_ln: &[u8], symbol: u8) -> std::io::Result<()> {
        self.left_ln_buf.clear();
        self.right_ln_buf.clear();
        let mut left_formatter = LineFormatter::new(&self.config, &mut self.left_ln_buf);
        let mut right_line_formatter = LineFormatter::new(&self.config, &mut self.right_ln_buf);
        let white_space_gutter = symbol == b' ';
        let half_width = self.config.sdiff_half_width;
        let column_two_offset = self.config.sdiff_column_two_offset;
        let separator_pos = self.config.separator_pos;
        let output = &mut self.output;
        let mut put_new_line = false;

        // this also evolves the separator |, but we can ignore it
        // at this point, since we don't have the | sep (yet)
        if !left_ln.is_empty() {
            put_new_line = put_new_line || (left_ln.last() == Some(&b'\n'));
        }
        if !right_ln.is_empty() {
            put_new_line = put_new_line || (right_ln.last() == Some(&b'\n'));
        }

        left_formatter.process_half_line(left_ln, half_width, false, white_space_gutter)?;
        right_line_formatter.process_half_line(right_ln, half_width, true, white_space_gutter)?;

        output.write_all(&self.left_ln_buf)?;
        if symbol != b' ' {
            // the diff always want to put all tabs possible in the usable are,
            // even in the middle space between the gutters if possible.

            let mut separator_buffer = vec![];
            let mut separator_formatter = LineFormatter::new(&self.config, &mut separator_buffer);
            separator_formatter.format_tabs_and_spaces(separator_pos + 1, column_two_offset);

            output.write_all(&[symbol])?;
            output.write_all(&separator_buffer)?;
        }
        output.write_all(&self.right_ln_buf)?;

        if put_new_line {
            writeln!(output)?;
        }

        Ok(())
    }
}

pub fn diff(from_file: &[u8], to_file: &[u8]) -> Vec<u8> {
    //      ^ The left file  ^ The right file

    let mut output = stdout().lock();
    let mut left_lines: Vec<&[u8]> = from_file.split(|&c| c == b'\n').collect();
    let mut right_lines: Vec<&[u8]> = to_file.split(|&c| c == b'\n').collect();
    let config = Config::new(FULL_WIDTH, TAB_SIZE, false);
    let mut output_handler = OutputHandler::new(config, &mut output);
    
    if left_lines.last() == Some(&&b""[..]) {
        left_lines.pop();
    }

    if right_lines.last() == Some(&&b""[..]) {
        right_lines.pop();
    }

    /*
    DISCLAIMER:
    Currently the diff engine does not produce results like the diff engine used in GNU diff,
    so some results may be inaccurate. For example, the line difference marker "|", according
    to the GNU documentation, appears when the same lines (only the actual line, although the
    relative line may change the result, so occasionally '|' markers appear with the same lines)
    are different but exist in both files. In the current solution the same result cannot be
    obtained because the diff engine does not return Both if both exist but are different,
    but instead returns a Left and a Right for each one, implying that two lines were added
    and deleted. Furthermore, the GNU diff program apparently stores some internal state
    (this internal state is just a note about how the diff engine works) about the lines.
    For example, an added or removed line directly counts in the line query of the original
    lines to be printed in the output. Because of this imbalance caused by additions and
    deletions, the characters ( and ) are introduced. They basically represent lines without
    context, which have lost their pair in the other file due to additions or deletions. Anyway,
    my goal with this disclaimer is to warn that for some reason, whether it's the diff engine's
    inability to determine and predict/precalculate the result of GNU's sdiff, with this software it's
    not possible to reproduce results that are 100% faithful to GNU's, however, the basic premise
    e of side diff of showing added and removed lines and creating edit scripts is totally possible.
    More studies are needed to cover GNU diff side by side with 100% accuracy, which is one of
    the goals of this project : )
    */
    for result in diff::slice(&left_lines, &right_lines) {
        match result {
            Result::Left(left_ln) => output_handler.push_output(left_ln, b"", b'<').unwrap(),
            Result::Right(right_ln) => output_handler.push_output(b"", right_ln, b'>').unwrap(),
            Result::Both(left_ln, right_ln) => {
                output_handler.push_output(left_ln, right_ln, b' ').unwrap()
            }
        }
    }

    vec![]
}

#[cfg(test)]
mod tests {
    const DEF_TAB_SIZE: usize = 4;

    use super::*;

    mod format_tabs_and_spaces {
        use super::*;

        const CONFIG_E_T: Config = Config {
            sdiff_half_width: 60,
            tab_size: DEF_TAB_SIZE,
            expanded: true,
            sdiff_column_two_offset: 0,
            separator_pos: 0,
        };

        const CONFIG_E_F: Config = Config {
            sdiff_half_width: 60,
            tab_size: DEF_TAB_SIZE,
            expanded: false,
            sdiff_column_two_offset: 0,
            separator_pos: 0,
        };

        fn build_ln_formatter(expanded: bool, buf: &mut Vec<u8>) -> LineFormatter {
            if expanded {
                LineFormatter {
                    config: &CONFIG_E_T,
                    buf,
                }
            } else {
                LineFormatter {
                    config: &CONFIG_E_F,
                    buf,
                }
            }
        }

        #[test]
        fn test_format_tabs_and_spaces_expanded_false() {
            let mut buf = vec![];
            let mut formatter = build_ln_formatter(false, &mut buf);
            formatter.format_tabs_and_spaces(0, 5);
            assert_eq!(buf, vec![b'\t', b' ']);
        }

        #[test]
        fn test_format_tabs_and_spaces_expanded_true() {
            let mut buf = vec![];
            let mut formatter = build_ln_formatter(true, &mut buf);
            formatter.format_tabs_and_spaces(0, 5);
            assert_eq!(buf, vec![b' '; 5]);
        }

        #[test]
        fn test_format_tabs_and_spaces_from_greater_than_to() {
            let mut buf = vec![];
            let mut formatter = build_ln_formatter(false, &mut buf);
            formatter.format_tabs_and_spaces(6, 5);
            assert!(buf.is_empty());
        }

        #[test]
        fn test_format_from_non_zero_position() {
            let mut buf = vec![];
            let mut formatter = build_ln_formatter(false, &mut buf);
            formatter.format_tabs_and_spaces(2, 7);
            assert_eq!(buf, vec![b'\t', b' ', b' ', b' ']);
        }

        #[test]
        fn test_multiple_full_tabs_needed() {
            let mut buf = vec![];
            let mut formatter = build_ln_formatter(false, &mut buf);
            formatter.format_tabs_and_spaces(0, 12);
            assert_eq!(buf, vec![b'\t', b'\t', b'\t']);
        }

        #[test]
        fn test_uneven_tab_boundary_with_spaces() {
            let mut buf = vec![];
            let mut formatter = build_ln_formatter(false, &mut buf);
            formatter.format_tabs_and_spaces(3, 10);
            assert_eq!(buf, vec![b'\t', b'\t', b' ', b' ']);
        }

        #[test]
        fn test_expanded_true_with_offset() {
            let mut buf = vec![];
            let mut formatter = build_ln_formatter(true, &mut buf);
            formatter.format_tabs_and_spaces(3, 9);
            assert_eq!(buf, vec![b' '; 6]);
        }

        #[test]
        fn test_exact_tab_boundary_from_midpoint() {
            let mut buf = vec![];
            let mut formatter = build_ln_formatter(false, &mut buf);
            formatter.format_tabs_and_spaces(4, 8);
            assert_eq!(buf, vec![b'\t']);
        }

        #[test]
        fn test_mixed_tabs_and_spaces_edge_case() {
            let mut buf = vec![];
            let mut formatter = build_ln_formatter(false, &mut buf);
            formatter.format_tabs_and_spaces(5, 9);
            assert_eq!(buf, vec![b'\t', b' ']);
        }

        #[test]
        fn test_minimal_gap_with_tab() {
            let mut buf = vec![];
            let mut formatter = build_ln_formatter(false, &mut buf);
            formatter.format_tabs_and_spaces(7, 8);
            assert_eq!(buf, vec![b'\t']);
        }

        #[test]
        fn test_expanded_false_with_tab_at_end() {
            let mut buf = vec![];
            let mut formatter = build_ln_formatter(false, &mut buf);
            formatter.format_tabs_and_spaces(6, 8);
            assert_eq!(buf, vec![b'\t']);
        }
    }

    mod process_half_line {
        use super::*;

        fn create_test_config(expanded: bool, tab_size: usize) -> Config {
            Config {
                sdiff_half_width: 30,
                sdiff_column_two_offset: 60,
                tab_size,
                expanded,
                separator_pos: 15,
            }
        }

        #[test]
        fn test_empty_line_left_expanded_false() {
            let config = create_test_config(false, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            formatter.process_half_line(b"", 10, false, false).unwrap();
            assert_eq!(buf.len(), 5);
            assert_eq!(buf, vec![b'\t', b'\t', b' ', b' ', b' ']);
        }

        #[test]
        fn test_tabs_unexpanded() {
            let config = create_test_config(false, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            formatter
                .process_half_line(b"\tabc", 8, false, false)
                .unwrap();
            assert_eq!(buf, vec![b'\t', b'a', b'b', b'c', b'\t', b' ']);
        }

        #[test]
        fn test_utf8_multibyte() {
            let config = create_test_config(false, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let s = "😉😉😉".as_bytes();
            formatter.process_half_line(s, 3, false, false).unwrap();
            let mut r = vec![];
            r.write_all("😉\t".as_bytes()).unwrap();
            assert_eq!(buf, r)
        }

        #[test]
        fn test_newline_handling() {
            let config = create_test_config(false, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            formatter
                .process_half_line(b"abc\ndef", 5, false, false)
                .unwrap();
            assert_eq!(buf, vec![b'a', b'b', b'c', b'\t', b' ', b' ']);
        }

        #[test]
        fn test_carriage_return() {
            let config = create_test_config(false, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            formatter
                .process_half_line(b"\rxyz", 5, true, false)
                .unwrap();
            let mut r = vec![b'\r'];
            r.extend(vec![b'\t'; 15]);
            r.extend(vec![b'x', b'y', b'z']);
            assert_eq!(buf, r);
        }

        #[test]
        fn test_exact_width_fit() {
            let config = create_test_config(true, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            formatter
                .process_half_line(b"abcd", 4, false, false)
                .unwrap();
            assert_eq!(buf.len(), 5);
            assert_eq!(buf, b"abcd ".to_vec());
        }

        #[test]
        fn test_non_utf8_bytes() {
            let config = create_test_config(false, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            // ISO-8859-1
            formatter
                .process_half_line(&[0x63, 0x61, 0x66, 0xE9], 5, false, false)
                .unwrap();
            assert_eq!(&buf, &[0x63, 0x61, 0x66, 0xE9, b' ', b' ']);
            assert!(String::from_utf8(buf).is_err());
        }

        #[test]
        fn test_non_utf8_bytes_ignore_padding_bytes() {
            let config = create_test_config(false, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let utf32le_bytes = [
                0x63, 0x00, 0x00, 0x00, // 'c'
                0x61, 0x00, 0x00, 0x00, // 'a'
                0x66, 0x00, 0x00, 0x00, // 'f'
                0xE9, 0x00, 0x00, 0x00, // 'é'
            ];
            // utf8 little endiand 32 bits (or 4 bytes per char)
            formatter
                .process_half_line(&utf32le_bytes, 6, false, false)
                .unwrap();
            let mut r = utf32le_bytes.to_vec();
            r.extend(vec![b' '; 3]);
            assert_eq!(buf, r);
        }

        #[test]
        fn test_non_utf8_non_preserve_ascii_bytes_cut() {
            let config = create_test_config(false, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let gb18030 = b"\x63\x61\x66\xA8\x80"; // some random chinese encoding
            //                                   ^ é char, start multi byte
            formatter
                .process_half_line(gb18030, 4, false, false)
                .unwrap();
            assert_eq!(buf, b"\x63\x61\x66\xA8 "); // break the encoding of 'é' letter
        }

        #[test]
        fn test_right_line_padding() {
            let config = create_test_config(false, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            formatter.process_half_line(b"xyz", 5, true, true).unwrap();
            assert_eq!(buf.len(), 3);
        }

        #[test]
        fn test_mixed_tabs_spaces() {
            let config = create_test_config(false, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            formatter
                .process_half_line(b"\t  \t", 10, false, false)
                .unwrap();
            assert_eq!(buf, vec![b'\t', b' ', b' ', b'\t', b' ', b' ', b' ']);
        }

        #[test]
        fn test_overflow_multibyte() {
            let config = create_test_config(false, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let s = "日本語".as_bytes();
            formatter.process_half_line(s, 5, false, false).unwrap();
            assert_eq!(buf, "日本  ".as_bytes());
        }

        #[test]
        fn test_white_space_gutter() {
            let config = create_test_config(false, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let s = b"abc";
            formatter.process_half_line(s, 3, false, true).unwrap();
            assert_eq!(buf, b"abc\t  ");
        }
        
        #[test]
        fn test_expanded_true() {
            let config = create_test_config(true, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let s = b"abc";
            formatter.process_half_line(s, 10, false, false).unwrap();
            assert_eq!(buf, b"abc        ")
        }
        
        #[test]
        fn test_expanded_true_with_gutter() {
            let config = create_test_config(true, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let s = b"abc";
            formatter.process_half_line(s, 10, false, true).unwrap();
            assert_eq!(buf, b"abc          ")
        }
        
        #[test]
        fn test_width0_chars() {
            let config = create_test_config(false, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let s = b"abc\0\x0B\x07\x0C";
            formatter.process_half_line(s, 4, false, false).unwrap();
            assert_eq!(buf, b"abc\0\x0B\x07\x0C\t ")
        }
        
        #[test]
        fn test_left_empty_white_space_gutter() {
            let config = create_test_config(false, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let s = b"";
            formatter.process_half_line(s, 9, false, true).unwrap();
            assert_eq!(buf, b"\t\t\t");
        }

        #[test]
        fn test_s_size_eq_max_width_p1() {
            let config = create_test_config(false, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let s = b"abcdefghij";
            formatter.process_half_line(s, 10, false, false).unwrap();
            assert_eq!(buf, b"abcdefghij ");
        }

        #[test]
        fn test_mixed_tabs_and_spaces_inversion() {
            let config = create_test_config(false, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let s = b" \t \t ";
            formatter.process_half_line(s, 10, false, false).unwrap();
            assert_eq!(buf, b" \t \t   ");
        }

        #[test]
        fn test_expanded_with_tabs() {
            let config = create_test_config(true, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let s = b" \t \t ";
            formatter.process_half_line(s, 10, false, false).unwrap();
            assert_eq!(buf, b"           ");
        }

        #[test]
        fn test_expanded_with_tabs_and_space_gutter() {
            let config = create_test_config(true, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let s = b" \t \t ";
            formatter.process_half_line(s, 10, false, true).unwrap();
            assert_eq!(buf, b"             ");
        }

        #[test]
        fn test_zero_width_unicode_chars() {
            let config = create_test_config(false, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let s = "\u{200B}".as_bytes();
            formatter.process_half_line(s, 10, false, false).unwrap();
            assert_eq!(buf, "\u{200B}\t\t   ".as_bytes());
        }

        #[test]
        fn test_multiple_carriage_returns() {
            let config = create_test_config(false, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let s = b"\r\r";
            formatter.process_half_line(s, 10, false, false).unwrap();
            let mut r = vec![b'\r'];
            r.extend(vec![b'\t'; 15]);
            r.push(b'\r');
            r.extend(vec![b'\t'; 15]);
            r.extend(vec![b'\t'; 2]);
            r.extend(vec![b' '; 3]);
            assert_eq!(buf, r);
        }

        #[test]
        fn test_multiple_carriage_returns_is_right_true() {
            let config = create_test_config(false, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let s = b"\r\r";
            formatter.process_half_line(s, 10, true, false).unwrap();
            let mut r = vec![b'\r'];
            r.extend(vec![b'\t'; 15]);
            r.push(b'\r');
            r.extend(vec![b'\t'; 15]);
            assert_eq!(buf, r);
        }

        #[test]
        fn test_mixed_invalid_utf8_with_valid() {
            let config = create_test_config(false, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let s = b"abc\xFF\xFEdef";
            formatter.process_half_line(s, 10, false, false).unwrap();
            assert!(String::from_utf8(s.to_vec()).is_err());
            assert_eq!(buf, b"abc\xFF\xFEdef   ");
        }
        
        #[test]
        fn test_max_width_zero() {
            let config = create_test_config(false, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let s = b"foo bar";
            formatter.process_half_line(s, 0, false, false).unwrap();
            assert_eq!(buf, vec![b' ']);
        }
        
        #[test]
        fn test_line_only_with_tabs() {
            let config = create_test_config(false, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let s = b"\t\t\t";
            formatter.process_half_line(s, 10, false, false).unwrap();
            assert_eq!(buf, vec![b'\t', b'\t', b' ', b' ', b' '])
        }

        #[test]
        fn test_tabs_expanded() {
            let config = create_test_config(true, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let s = b"\t\t\t";
            formatter.process_half_line(s, 12, false, false).unwrap();
            assert_eq!(buf, b" ".repeat(13));
        }

        #[test]
        fn test_mixed_tabs() {
            let config = create_test_config(false, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let s = b"a\tb\tc\t";
            formatter.process_half_line(s, 10, false, false).unwrap();
            assert_eq!(buf, b"a\tb\tc  ");
        }

        #[test]
        fn test_mixed_tabs_with_gutter() {
            let config = create_test_config(false, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let s = b"a\tb\tc\t";
            formatter.process_half_line(s, 10, false, true).unwrap();
            assert_eq!(buf, b"a\tb\tc\t ");
        }

        #[test]
        fn test_mixed_tabs_expanded() {
            let config = create_test_config(true, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let s = b"a\tb\tc\t";
            formatter.process_half_line(s, 10, false, false).unwrap();
            assert_eq!(buf, b"a   b   c  ");
        }

        #[test]
        fn test_mixed_tabs_expanded_with_gutter() {
            let config = create_test_config(true, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let s = b"a\tb\tc\t";
            formatter.process_half_line(s, 10, false, true).unwrap();
            assert_eq!(buf, b"a   b   c    ");
        }
        
        #[test]
        fn test_break_if_invalid_max_width() {
            let config = create_test_config(true, DEF_TAB_SIZE);
            let mut buf = vec![];
            let mut formatter = LineFormatter::new(&config, &mut buf);
            let s = b"a\tb\tc\t";
            formatter.process_half_line(s, 61, false, true).unwrap();
            assert_eq!(buf, b"");
            assert_eq!(buf.len(), 0);
        }
    }
}
