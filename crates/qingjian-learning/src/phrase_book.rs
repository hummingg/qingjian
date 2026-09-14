//! English Coach 短语的学习记录与间隔重复（SRS）。
//!
//! 被动学习（用户在输入法里看到一句英文附带的短语）只记 exposure，不推进复习间隔；
//! 主动学习（用户进复习界面答题）走 SM-2 调度 `next_review_at`。
//!
//! 落盘一个 TSV：`phrase\tseen\tattempts\tcorrect\tlast_seen\tnext_review\tinterval\tease`，
//! 坏行跳过，读不了就只在内存里记——输入链路不依赖这里的任何结果。

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use jiff::civil::Date;
use qingjian_core::storage::{read_text_lossy, write_atomic_str};

use crate::error::LearningError;

/// SM-2 的初始 ease factor。
const INITIAL_EASE: f32 = 2.5;
/// ease factor 下限：再差也不能让间隔缩到原地踏步。
const MIN_EASE: f32 = 1.3;
/// ease factor 上限。
const MAX_EASE: f32 = 2.5;
/// 第一次答对后的间隔（天）。
const FIRST_INTERVAL: u32 = 1;
/// 第二次答对后的间隔（天）。
const SECOND_INTERVAL: u32 = 6;

/// 一条短语的被动曝光 + 主动复习记录。
#[derive(Debug, Clone, PartialEq)]
struct Entry {
    /// 在输入法面板里看到过几次（被动）。
    seen: u32,

    /// 主动复习过几次。
    attempts: u32,

    /// 主动复习答对几次。
    correct: u32,

    /// 最近一次记录（曝光或复习）的日期。
    last_seen: Date,

    /// 下次该复习的日期；从没复习过就等于 `last_seen`（不催）。
    next_review: Date,

    /// 当前复习间隔（天）。
    interval: u32,

    /// SM-2 的 ease factor。
    ease: f32,

    /// 最近一次 AI 给的中文释义，复习时显示；空就是还没收到过。
    meaning: String,
}

impl Entry {
    fn new(date: Date) -> Self {
        Self {
            seen: 0,
            attempts: 0,
            correct: 0,
            last_seen: date,
            next_review: date,
            interval: 0,
            ease: INITIAL_EASE,
            meaning: String::new(),
        }
    }

    /// 掌握度：复习过的按正确率，没复习过的按曝光次数渐近（3 次曝光算熟）。
    fn mastery(&self) -> f32 {
        if self.attempts > 0 {
            self.correct as f32 / self.attempts as f32
        } else {
            (self.seen as f32 / 3.0).min(1.0)
        }
    }
}

/// English Coach 短语本：一个短语一行，按键排好，写在本机数据目录里。
#[derive(Debug, Default)]
pub struct PhraseBook {
    entries: BTreeMap<String, Entry>,
    dirty: bool,
    path: Option<PathBuf>,
}

impl PhraseBook {
    /// 从文件加载（不存在就从零开始）；读不了就退回只在内存里记。
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        match read_text_lossy(&path) {
            Ok(text) => Self {
                entries: text.as_deref().map(parse).unwrap_or_default(),
                dirty: false,
                path: Some(path),
            },
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "短语本读不了，本次只在内存里记");
                Self::default()
            }
        }
    }

    /// 只在内存里记（没文件路径，`save` 是空操作）。
    pub fn in_memory() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 记一次被动曝光（面板上看到了这条短语）。不推进复习间隔。
    /// 带上 AI 给的释义，复习时能显示；空字符串不覆盖已有的。
    pub fn record_exposure_on(&mut self, phrase: &str, meaning: &str, date: Date) {
        self.dirty = true;
        let entry = self
            .entries
            .entry(phrase.to_owned())
            .or_insert_with(|| Entry::new(date));
        entry.seen += 1;
        entry.last_seen = date;
        if !meaning.is_empty() {
            entry.meaning = meaning.to_owned();
        }
    }

    /// 记一次主动复习。`quality` 是 SM-2 的 0–5 分（>=3 算答对）。
    /// 返回更新后的掌握度，方便复习界面当场刷新。
    pub fn record_review_on(&mut self, phrase: &str, quality: u8, date: Date) -> f32 {
        self.dirty = true;
        let entry = self
            .entries
            .entry(phrase.to_owned())
            .or_insert_with(|| Entry::new(date));
        entry.attempts += 1;
        entry.last_seen = date;
        if quality >= 3 {
            entry.correct += 1;
            entry.interval = match entry.interval {
                0 => FIRST_INTERVAL,
                FIRST_INTERVAL => SECOND_INTERVAL,
                rest => (rest as f32 * entry.ease).round() as u32,
            };
            entry.ease = (entry.ease + 0.1).min(MAX_EASE);
        } else {
            entry.interval = FIRST_INTERVAL;
            entry.ease = (entry.ease - 0.2).max(MIN_EASE);
        }
        entry.next_review = date.saturating_add(jiff::Span::new().days(entry.interval as i64));
        entry.mastery()
    }

    /// 到了该复习的短语（`next_review <= today`），按到期先后排。
    pub fn due_on(&self, today: Date) -> Vec<(String, f32)> {
        self.entries
            .iter()
            .filter(|(_, entry)| entry.next_review <= today && entry.attempts > 0)
            .map(|(phrase, entry)| (phrase.clone(), entry.mastery()))
            .collect()
    }

    /// 到期且带释义的短语，复习窗口用：`(phrase, meaning, mastery)`。
    pub fn due_with_meaning_on(&self, today: Date) -> Vec<(String, String, f32)> {
        self.entries
            .iter()
            .filter(|(_, entry)| entry.next_review <= today && entry.attempts > 0)
            .map(|(phrase, entry)| (phrase.clone(), entry.meaning.clone(), entry.mastery()))
            .collect()
    }

    /// 一条短语的掌握度（没记过就是 0.0）。
    pub fn mastery(&self, phrase: &str) -> f32 {
        self.entries.get(phrase).map_or(0.0, Entry::mastery)
    }

    /// 写回文件（没新记录就什么都不做）。
    pub fn save(&mut self) -> Result<(), LearningError> {
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        if !self.dirty {
            return Ok(());
        }
        self.save_to(&path)?;
        self.dirty = false;
        Ok(())
    }

    fn save_to(&self, path: &Path) -> Result<(), LearningError> {
        let mut text = String::from(
            "# phrase\tseen\tattempts\tcorrect\tlast_seen\tnext_review\tinterval\tease\tmeaning\n",
        );
        for (phrase, entry) in &self.entries {
            let _ = writeln!(
                text,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.2}\t{}",
                phrase,
                entry.seen,
                entry.attempts,
                entry.correct,
                entry.last_seen,
                entry.next_review,
                entry.interval,
                entry.ease,
                entry.meaning
            );
        }
        write_atomic_str(path, &text)?;
        Ok(())
    }
}

fn today() -> Date {
    jiff::Zoned::now().date()
}

impl PhraseBook {
    /// 以本机今天为准的便捷方法。
    pub fn record_exposure(&mut self, phrase: &str, meaning: &str) {
        self.record_exposure_on(phrase, meaning, today());
    }

    /// 以本机今天为准的便捷方法。
    pub fn record_review(&mut self, phrase: &str, quality: u8) -> f32 {
        self.record_review_on(phrase, quality, today())
    }

    /// 以本机今天为准的到期列表。
    pub fn due(&self) -> Vec<(String, f32)> {
        self.due_on(today())
    }

    /// 以本机今天为准的到期列表（带释义）。
    pub fn due_with_meaning(&self) -> Vec<(String, String, f32)> {
        self.due_with_meaning_on(today())
    }
}

fn parse(text: &str) -> BTreeMap<String, Entry> {
    let mut entries = BTreeMap::new();
    for (number, line) in text.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match parse_line(line) {
            Some((phrase, entry)) => {
                entries.insert(phrase, entry);
            }
            None => tracing::warn!(line = number + 1, "短语本有坏行，跳过"),
        }
    }
    entries
}

fn parse_line(line: &str) -> Option<(String, Entry)> {
    let mut fields = line.split('\t');
    let phrase = fields.next()?.to_owned();
    if phrase.is_empty() {
        return None;
    }
    let seen: u32 = fields.next()?.parse().ok()?;
    let attempts: u32 = fields.next()?.parse().ok()?;
    let correct: u32 = fields.next()?.parse().ok()?;
    let last_seen: Date = fields.next()?.parse().ok()?;
    let next_review: Date = fields.next()?.parse().ok()?;
    let interval: u32 = fields.next()?.parse().ok()?;
    let ease: f32 = fields.next()?.parse().ok()?;
    // meaning 是最后加的列，老文件没有就留空
    let meaning = fields.next().unwrap_or("").to_owned();
    Some((
        phrase,
        Entry {
            seen,
            attempts,
            correct,
            last_seen,
            next_review,
            interval,
            ease,
            meaning,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &str) -> Date {
        text.parse().unwrap()
    }

    #[test]
    fn exposure_accumulates_without_scheduling_review() {
        let mut book = PhraseBook::default();
        book.record_exposure_on("get back to", "回复某人", date("2026-09-01"));
        book.record_exposure_on("get back to", "回复某人", date("2026-09-02"));
        assert_eq!(book.mastery("get back to"), 2.0 / 3.0);
        // 没复习过，due 里不出现
        assert!(book.due_on(date("2026-09-02")).is_empty());
    }

    #[test]
    fn correct_review_advances_interval_and_eases() {
        let mut book = PhraseBook::default();
        let d0 = date("2026-09-01");
        // 第一次答对：间隔 1 天，9-02 到期
        book.record_review_on("figure out", 5, d0);
        let due = book.due_on(date("2026-09-02"));
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].0, "figure out");
        // 第二次答对：间隔 6 天
        book.record_review_on("figure out", 5, date("2026-09-02"));
        assert!(book.due_on(date("2026-09-07")).is_empty());
        assert_eq!(book.due_on(date("2026-09-08")).len(), 1);
    }

    #[test]
    fn wrong_review_resets_interval_and_shrinks_ease() {
        let mut book = PhraseBook::default();
        book.record_review_on("make sense", 5, date("2026-09-01"));
        book.record_review_on("make sense", 5, date("2026-09-02")); // 间隔 6
        book.record_review_on("make sense", 2, date("2026-09-03")); // 答错，间隔回 1
        let due = book.due_on(date("2026-09-04"));
        assert_eq!(due.len(), 1);
        assert!(due[0].1 < 1.0); // 正确率 2/3
    }

    #[test]
    fn round_trips_through_file() {
        let dir = std::env::temp_dir().join(format!("qingjian-phrases-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("coach-phrases.tsv");
        std::fs::write(
            &path,
            "# 头\nget back to\t2\t1\t1\t2026-09-01\t2026-09-02\t1\t2.50\n坏行\n\t1\t0\t0\t2026-09-01\t2026-09-01\t0\t2.50\n",
        )
        .unwrap();
        let mut book = PhraseBook::open(&path);
        assert_eq!(book.len(), 1);
        book.record_exposure_on("get back to", "回复某人", date("2026-09-03"));
        book.record_review_on("work on", 4, date("2026-09-03"));
        book.save().unwrap();
        let reloaded = PhraseBook::open(&path);
        assert_eq!(reloaded.mastery("work on"), 1.0);
        assert!(
            reloaded
                .due_on(date("2026-09-04"))
                .iter()
                .any(|(p, _)| p == "work on")
        );
        // meaning 也能读回来
        let due = reloaded.due_with_meaning_on(date("2026-09-04"));
        assert!(
            due.iter()
                .any(|(p, m, _)| p == "get back to" && m == "回复某人")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
