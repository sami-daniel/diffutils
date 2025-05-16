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
    white_space_gutter: bool,
    buf: &mut Vec<u8>,
) -> std::io::Result<()> {
    let mut current_width = 0;
    let mut is_utf8 = false;
    let iter = s.iter();
    let input = match String::from_utf8(s.to_vec()) {
        // third buffer
        Ok(s) => {
            is_utf8 = true;
            s
        }
        Err(_) => String::new(),
    };

    if !white_space_gutter && is_right {
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
    if !is_right && !s.is_empty() {
        format_tabs_and_spaces(
            current_width,
            max_width + if white_space_gutter { 3 } else { 1 },
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
        if symbol == b' ' { true } else { false },
        left_ln_buffer,
    )?;

    process_half_line(
        right_ln,
        modifiers.sdiff_half_width,
        modifiers.expanded,
        modifiers.tab_size,
        true,
        if symbol == b' ' { true } else { false },
        right_ln_buffer,
    )?;

    output.write_all(left_ln_buffer)?;
    if symbol != b' ' {
        output.write_all(&[symbol])?;
    }
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

    // GNU diff uses this calculation to calculate the size of a half line
    // based on the options passed (like -w, -t, etc.). It's actually
    // pretty useless, because we (actually) don't have any size modifiers
    // that can change this, however I just want to leave the calculatio
    // n here, since it's not clear and may cause some confusion

    let t = /* expanded_tabs ? 1 : tabsize, however, we dont have -t opt, so it will be always 8 */ 8;
    let t_plus_g = t + GUTTER_WIDTH_MIN;
    let unaligned_off = (C_WIDTH >> 1) + (t_plus_g >> 1) + (C_WIDTH & t_plus_g & 1);
    let off = unaligned_off - unaligned_off % t;
    let sdiff_half_width = (max(0, min(off - GUTTER_WIDTH_MIN, C_WIDTH - off))) as usize;

    let mut left_ln_buf = Vec::with_capacity(sdiff_half_width);
    let mut right_ln_buf = Vec::with_capacity(sdiff_half_width);

    let modifiers = Modifiers::new(sdiff_half_width, t as usize, false);

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
    not possible to reproduce results that are 100% faithful to GNU's, however, the basic premis
    e of side diff of showing added and removed lines and creating edit scripts is totally possible.
    More studies are needed to cover GNU diff side by side with 100% accuracy, which is one of
    the goals of this project : )
    */
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

    mod format_tabs_and_spaces {
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

    mod process_half_line {
        use super::*;

        #[test]
        fn test_expanded_tabs_with_offset() {
            let mut buf = Vec::new();
            // "a\tb" com tab_size=4, expanded=true, max_width=10
            process_half_line(b"a\tb", 10, true, 4, false, false, &mut buf).unwrap();
            assert_eq!(buf, b"a   b      "); // a + 3 espaços (preenche tab) + b + 3 espaços (alinhamento)
        }

        #[test]
        fn test_tab_at_end_of_line() {
            let mut buf = Vec::new();
            // Linha termina com tab (expanded=false)
            process_half_line(b"text\t", 8, false, 4, false, false, &mut buf).unwrap();
            assert_eq!(buf, b"text\t  "); // text (4) + tab (4) + 2 espaços (total 8)
        }

        // Testes para UTF-8 complexo
        #[test]
        fn test_utf8_combining_characters() {
            let mut buf = Vec::new();
            // "café" (e + combining acute accent, largura total 4)
            let input = "cafe\u{301}";
            process_half_line(input.as_bytes(), 5, false, 4, false, false, &mut buf).unwrap();
            assert_eq!(buf, input.as_bytes()); // Não truncado, largura total 4
        }

        #[test]
        fn test_utf8_zero_width_char() {
            let mut buf = Vec::new();
            // Caractere de largura zero (ex: 'à' com combining mark)
            let input = "a\u{304}"; // a + combining macron
            process_half_line(input.as_bytes(), 3, false, 4, false, false, &mut buf).unwrap();
            assert_eq!(buf.len(), 3); // Considerado como 1 caractere de largura
        }

        // Testes para truncamento preciso
        #[test]
        fn test_truncate_at_exact_width() {
            let mut buf = Vec::new();
            process_half_line(b"12345", 5, false, 4, false, false, &mut buf).unwrap();
            assert_eq!(buf, b"12345"); // Preenche exatamente o width
        }

        #[test]
        fn test_truncate_with_tab_at_edge() {
            let mut buf = Vec::new();
            // Tab no limite do truncamento (max_width=8)
            process_half_line(b"1234\t", 8, false, 4, false, false, &mut buf).unwrap();
            assert_eq!(buf, b"1234\t  "); // 4 + tab (4) + 2 espaços
        }

        // Testes para linhas direitas (right_side)
        #[test]
        fn test_right_line_with_leading_space() {
            let mut buf = Vec::new();
            process_half_line(b"right", 10, false, 4, true, false, &mut buf).unwrap();
            assert_eq!(buf[0], b' '); // Espaço inicial adicionado
            assert_eq!(buf.len(), 10 + 1); // Espaço + conteúdo + padding
        }

        #[test]
        fn test_right_line_empty_but_flagged() {
            let mut buf = Vec::new();
            process_half_line(b"", 10, false, 4, true, false, &mut buf).unwrap();
            assert!(buf.is_empty()); // Não adiciona padding para linha direita vazia
        }

        // Testes para caracteres de controle
        #[test]
        fn test_ignore_carriage_return() {
            let mut buf = Vec::new();
            process_half_line(b"line\r\n", 10, false, 4, false, false, &mut buf).unwrap();
            assert!(!buf.contains(&b'\r')); // \r é ignorado
        }

        #[test]
        fn test_newline_in_middle() {
            let mut buf = Vec::new();
            process_half_line(b"abc\ndef", 10, false, 4, false, false, &mut buf).unwrap();
            assert_eq!(buf, b"abc       "); // Para após \n
        }

        // Teste para entrada binária (não UTF-8)
        #[test]
        fn test_invalid_utf8_fallback() {
            let mut buf = Vec::new();
            let input = &[0xFF, 0xFE]; // Bytes inválidos em UTF-8
            process_half_line(input, 5, false, 4, false, false, &mut buf).unwrap();
            assert_eq!(buf, input); // Processa como bytes brutos
        }

        // Teste para alinhamento com tabs e espaços mistos
        #[test]
        fn test_mixed_tabs_and_spaces_alignment() {
            let mut buf = Vec::new();
            process_half_line(b"\t \t ", 10, false, 4, false, false, &mut buf).unwrap();
            assert_eq!(buf, b"\t \t   "); // Mantém tabs e espaços originais
        }

        // Teste para max_width zero (edge case)
        #[test]
        fn test_max_width_zero() {
            let mut buf = Vec::new();
            process_half_line(b"test", 0, false, 4, false, false, &mut buf).unwrap();
            assert!(buf.is_empty()); // Nada é processado
        }

        // Teste para tab_size maior que max_width
        #[test]
        fn test_tab_size_exceeds_max_width() {
            let mut buf = Vec::new();
            process_half_line(b"\t", 3, false, 8, false, false, &mut buf).unwrap();
            assert_eq!(buf, b"\t  "); // Tab ocupa 8, mas é truncado para 3
        }

        // Teste para linha com apenas espaços
        #[test]
        fn test_line_with_only_spaces() {
            let mut buf = Vec::new();
            process_half_line(b"    ", 4, false, 4, false, false, &mut buf).unwrap();
            assert_eq!(buf, b"    "); // Espaços são mantidos
        }
    }
}