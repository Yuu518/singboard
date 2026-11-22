use gpui::{FontStyle, FontWeight, HighlightStyle, SharedString, StyledText};
use std::ops::Range;

pub const FONT_FAMILY: &str = "Twemoji Mozilla";
pub const FONT_DATA: &[u8] = include_bytes!("../assets/TwemojiMozilla.ttf");

const VS15: char = '\u{FE0E}';
const VS16: char = '\u{FE0F}';
const ZWJ: char = '\u{200D}';
const KEYCAP: char = '\u{20E3}';
const BLACK_FLAG: char = '\u{1F3F4}';
const CANCEL_TAG: char = '\u{E007F}';

fn is_regional_indicator(c: char) -> bool {
    matches!(c, '\u{1F1E6}'..='\u{1F1FF}')
}

fn is_tag_char(c: char) -> bool {
    matches!(c, '\u{E0020}'..='\u{E007E}')
}

fn is_skin_tone(c: char) -> bool {
    matches!(c, '\u{1F3FB}'..='\u{1F3FF}')
}

fn is_keycap_base(c: char) -> bool {
    matches!(c, '#' | '*' | '0'..='9')
}

fn is_smp_emoji(c: char) -> bool {
    matches!(
        c,
        '\u{1F004}'
            | '\u{1F0CF}'
            | '\u{1F170}'..='\u{1F171}'
            | '\u{1F17E}'..='\u{1F17F}'
            | '\u{1F18E}'
            | '\u{1F191}'..='\u{1F19A}'
            | '\u{1F201}'..='\u{1F202}'
            | '\u{1F21A}'
            | '\u{1F22F}'
            | '\u{1F232}'..='\u{1F23A}'
            | '\u{1F250}'..='\u{1F251}'
            | '\u{1F300}'..='\u{1F321}'
            | '\u{1F324}'..='\u{1F393}'
            | '\u{1F396}'..='\u{1F397}'
            | '\u{1F399}'..='\u{1F39B}'
            | '\u{1F39E}'..='\u{1F3F0}'
            | '\u{1F3F3}'..='\u{1F3F5}'
            | '\u{1F3F7}'..='\u{1F4FD}'
            | '\u{1F4FF}'..='\u{1F53D}'
            | '\u{1F549}'..='\u{1F54E}'
            | '\u{1F550}'..='\u{1F567}'
            | '\u{1F56F}'..='\u{1F570}'
            | '\u{1F573}'..='\u{1F57A}'
            | '\u{1F587}'
            | '\u{1F58A}'..='\u{1F58D}'
            | '\u{1F590}'
            | '\u{1F595}'..='\u{1F596}'
            | '\u{1F5A4}'..='\u{1F5A5}'
            | '\u{1F5A8}'
            | '\u{1F5B1}'..='\u{1F5B2}'
            | '\u{1F5BC}'
            | '\u{1F5C2}'..='\u{1F5C4}'
            | '\u{1F5D1}'..='\u{1F5D3}'
            | '\u{1F5DC}'..='\u{1F5DE}'
            | '\u{1F5E1}'
            | '\u{1F5E3}'
            | '\u{1F5E8}'
            | '\u{1F5EF}'
            | '\u{1F5F3}'
            | '\u{1F5FA}'..='\u{1F64F}'
            | '\u{1F680}'..='\u{1F6C5}'
            | '\u{1F6CB}'..='\u{1F6D2}'
            | '\u{1F6D5}'..='\u{1F6D7}'
            | '\u{1F6DD}'..='\u{1F6E5}'
            | '\u{1F6E9}'
            | '\u{1F6EB}'..='\u{1F6EC}'
            | '\u{1F6F0}'
            | '\u{1F6F3}'..='\u{1F6FC}'
            | '\u{1F7E0}'..='\u{1F7EB}'
            | '\u{1F7F0}'
            | '\u{1F90C}'..='\u{1F93A}'
            | '\u{1F93C}'..='\u{1F945}'
            | '\u{1F947}'..='\u{1F9FF}'
            | '\u{1FA70}'..='\u{1FA74}'
            | '\u{1FA78}'..='\u{1FA7C}'
            | '\u{1FA80}'..='\u{1FA86}'
            | '\u{1FA90}'..='\u{1FAAC}'
            | '\u{1FAB0}'..='\u{1FABA}'
            | '\u{1FAC0}'..='\u{1FAC5}'
            | '\u{1FAD0}'..='\u{1FAD9}'
            | '\u{1FAE0}'..='\u{1FAE7}'
            | '\u{1FAF0}'..='\u{1FAF6}'
    )
}

fn is_bmp_default_emoji(c: char) -> bool {
    matches!(
        c,
        '\u{231A}'..='\u{231B}'
            | '\u{23E9}'..='\u{23F3}'
            | '\u{23F8}'..='\u{23FA}'
            | '\u{25FD}'..='\u{25FE}'
            | '\u{2600}'..='\u{2604}'
            | '\u{260E}'
            | '\u{2611}'
            | '\u{2614}'..='\u{2615}'
            | '\u{2618}'
            | '\u{261D}'
            | '\u{2620}'
            | '\u{2622}'..='\u{2623}'
            | '\u{2626}'
            | '\u{262A}'
            | '\u{262E}'..='\u{262F}'
            | '\u{2638}'..='\u{263A}'
            | '\u{2648}'..='\u{2653}'
            | '\u{2668}'
            | '\u{267B}'
            | '\u{267F}'
            | '\u{2692}'..='\u{2697}'
            | '\u{2699}'
            | '\u{269B}'..='\u{269C}'
            | '\u{26A0}'..='\u{26A1}'
            | '\u{26AA}'..='\u{26AB}'
            | '\u{26B0}'..='\u{26B1}'
            | '\u{26BD}'..='\u{26BE}'
            | '\u{26C4}'..='\u{26C5}'
            | '\u{26C8}'
            | '\u{26CE}'..='\u{26CF}'
            | '\u{26D1}'
            | '\u{26D3}'..='\u{26D4}'
            | '\u{26E9}'..='\u{26EA}'
            | '\u{26F0}'..='\u{26F5}'
            | '\u{26F7}'..='\u{26FA}'
            | '\u{26FD}'
            | '\u{2702}'
            | '\u{2705}'
            | '\u{2708}'..='\u{270D}'
            | '\u{270F}'
            | '\u{2712}'
            | '\u{2728}'
            | '\u{2744}'
            | '\u{2747}'
            | '\u{274C}'
            | '\u{274E}'
            | '\u{2753}'..='\u{2755}'
            | '\u{2757}'
            | '\u{2763}'..='\u{2764}'
            | '\u{2795}'..='\u{2797}'
            | '\u{27B0}'
            | '\u{27BF}'
            | '\u{2B1B}'..='\u{2B1C}'
            | '\u{2B50}'
            | '\u{2B55}'
    )
}

fn is_bmp_text_emoji(c: char) -> bool {
    matches!(
        c,
        '\u{00A9}'
            | '\u{00AE}'
            | '\u{203C}'
            | '\u{2049}'
            | '\u{2122}'
            | '\u{2139}'
            | '\u{2194}'..='\u{2199}'
            | '\u{21A9}'..='\u{21AA}'
            | '\u{2328}'
            | '\u{23CF}'
            | '\u{24C2}'
            | '\u{25AA}'..='\u{25AB}'
            | '\u{25B6}'
            | '\u{25C0}'
            | '\u{25FB}'..='\u{25FC}'
            | '\u{2640}'
            | '\u{2642}'
            | '\u{265F}'..='\u{2660}'
            | '\u{2663}'
            | '\u{2665}'..='\u{2666}'
            | '\u{267E}'
            | '\u{26A7}'
            | '\u{2714}'
            | '\u{2716}'
            | '\u{271D}'
            | '\u{2721}'
            | '\u{2733}'..='\u{2734}'
            | '\u{27A1}'
            | '\u{2934}'..='\u{2935}'
            | '\u{2B05}'..='\u{2B07}'
            | '\u{3030}'
            | '\u{303D}'
            | '\u{3297}'
            | '\u{3299}'
    )
}

fn is_zwj_component(c: char) -> bool {
    is_smp_emoji(c) || is_bmp_default_emoji(c) || is_bmp_text_emoji(c)
}

fn push_range(ranges: &mut Vec<Range<usize>>, start: usize, end: usize) {
    if let Some(last) = ranges.last_mut()
        && last.end == start
    {
        last.end = end;
    } else {
        ranges.push(start..end);
    }
}

fn consume_emoji_modifiers(chars: &[(usize, char)], mut i: usize) -> usize {
    if i < chars.len() && chars[i].1 == VS16 {
        i += 1;
    }
    if i < chars.len() && is_skin_tone(chars[i].1) {
        i += 1;
    }
    while i + 1 < chars.len() && chars[i].1 == ZWJ && is_zwj_component(chars[i + 1].1) {
        i += 2;
        if i < chars.len() && chars[i].1 == VS16 {
            i += 1;
        }
        if i < chars.len() && is_skin_tone(chars[i].1) {
            i += 1;
        }
    }
    i
}

pub fn emoji_ranges(text: &str) -> Vec<Range<usize>> {
    if text.is_ascii() {
        return Vec::new();
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (start, ch) = chars[i];
        if is_regional_indicator(ch)
            && i + 1 < chars.len()
            && is_regional_indicator(chars[i + 1].1)
        {
            let (end_idx, second) = chars[i + 1];
            push_range(&mut ranges, start, end_idx + second.len_utf8());
            i += 2;
            continue;
        }
        if ch == BLACK_FLAG && i + 1 < chars.len() && is_tag_char(chars[i + 1].1) {
            let mut j = i + 1;
            while j < chars.len() && is_tag_char(chars[j].1) {
                j += 1;
            }
            if j < chars.len() && chars[j].1 == CANCEL_TAG {
                let (end_idx, end_ch) = chars[j];
                push_range(&mut ranges, start, end_idx + end_ch.len_utf8());
                i = j + 1;
                continue;
            }
        }
        if is_keycap_base(ch) {
            if i + 2 < chars.len() && chars[i + 1].1 == VS16 && chars[i + 2].1 == KEYCAP {
                let (end_idx, end_ch) = chars[i + 2];
                push_range(&mut ranges, start, end_idx + end_ch.len_utf8());
                i += 3;
                continue;
            }
            if i + 1 < chars.len() && chars[i + 1].1 == KEYCAP {
                let (end_idx, end_ch) = chars[i + 1];
                push_range(&mut ranges, start, end_idx + end_ch.len_utf8());
                i += 2;
                continue;
            }
        }
        if is_smp_emoji(ch) || is_bmp_default_emoji(ch) {
            if i + 1 < chars.len() && chars[i + 1].1 == VS15 {
                i += 2;
                continue;
            }
            let next = consume_emoji_modifiers(&chars, i + 1);
            let (end_idx, end_ch) = chars[next - 1];
            push_range(&mut ranges, start, end_idx + end_ch.len_utf8());
            i = next;
            continue;
        }
        if is_bmp_text_emoji(ch) && i + 1 < chars.len() && chars[i + 1].1 == VS16 {
            let next = consume_emoji_modifiers(&chars, i + 1);
            let (end_idx, end_ch) = chars[next - 1];
            push_range(&mut ranges, start, end_idx + end_ch.len_utf8());
            i = next;
            continue;
        }
        i += 1;
    }
    ranges
}

pub fn text(text: impl Into<SharedString>) -> StyledText {
    let text: SharedString = text.into();
    let ranges = emoji_ranges(&text);
    let label = StyledText::new(text);
    if ranges.is_empty() {
        return label;
    }
    let style = HighlightStyle {
        font_weight: Some(FontWeight::NORMAL),
        font_style: Some(FontStyle::Normal),
        ..Default::default()
    };
    label
        .with_highlights(ranges.iter().cloned().map(|range| (range, style)))
        .with_font_family_overrides(ranges.into_iter().map(|range| (range, FONT_FAMILY.into())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract<'a>(input: &'a str) -> Vec<&'a str> {
        emoji_ranges(input).into_iter().map(|r| &input[r]).collect()
    }

    #[test]
    fn detects_flags_general_emojis_and_sequences() {
        assert_eq!(extract("🇭🇰 香港实验性 IEPL 专线 1"), vec!["🇭🇰"]);
        assert_eq!(extract("🚀 节点选择"), vec!["🚀"]);
        assert_eq!(extract("♻️ 自动选择"), vec!["♻️"]);
        assert_eq!(extract("♻ 自动选择"), vec!["♻"]);
        assert_eq!(extract("1️⃣ 一号线路"), vec!["1️⃣"]);
        assert_eq!(extract("🏴󠁧󠁢󠁥󠁮󠁧󠁿 英国节点"), vec!["🏴󠁧󠁢󠁥󠁮󠁧󠁿"]);
        assert_eq!(extract("👨‍💻 开发节点"), vec!["👨‍💻"]);
        assert_eq!(extract("❤️‍🔥 热门"), vec!["❤️‍🔥"]);
        assert_eq!(extract("🇭🇰 香港 → 🇯🇵 日本"), vec!["🇭🇰", "🇯🇵"]);
        assert_eq!(extract("🇭🇰🚀 合并区间"), vec!["🇭🇰🚀"]);
    }

    #[test]
    fn preserves_plain_ui_symbols_and_text_selectors() {
        assert!(extract("Singboard 123 #1 *test*").is_empty());
        assert!(extract("© 2026 Singboard · › → ↑ ↓").is_empty());
        assert!(extract("❤\u{FE0E} 纯文本心形").is_empty());
        assert_eq!(extract("©️ 彩色版权"), vec!["©️"]);
    }

    #[test]
    fn bundled_font_is_colr_v0_cpal_v0_twemoji_mozilla() {
        let data = FONT_DATA;
        assert!(data.len() > 1_000_000);
        let num_tables = u16::from_be_bytes([data[4], data[5]]) as usize;
        let mut colr_ver = None;
        let mut cpal_ver = None;
        for i in 0..num_tables {
            let off = 12 + i * 16;
            let tag = &data[off..off + 4];
            let table_off =
                u32::from_be_bytes([data[off + 8], data[off + 9], data[off + 10], data[off + 11]])
                    as usize;
            if tag == b"COLR" {
                colr_ver = Some(u16::from_be_bytes([data[table_off], data[table_off + 1]]));
            } else if tag == b"CPAL" {
                cpal_ver = Some(u16::from_be_bytes([data[table_off], data[table_off + 1]]));
            }
        }
        assert_eq!(colr_ver, Some(0));
        assert_eq!(cpal_ver, Some(0));
    }
}
