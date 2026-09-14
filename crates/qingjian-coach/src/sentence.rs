/// 一次「喂文本」的结果：文本变了没有、这句话是否完结。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Feed {
    /// 句子文本有变化（值得安排一次 AI 请求）。
    pub changed: bool,

    /// 命中了强触发（。！？；、换行），这句话说完了。
    pub completed: bool,
}

/// 当前正在累积的那句中文，及其异步请求的版本号。
///
/// - `sentence_id` 在句子边界处递增：AI 回来时对不上当前 id 就整条丢弃。
/// - `version` 在文本每次变化时递增：用户改了字，旧版本的结果一律作废。
///
/// 两条校验合起来保证「旧 AI 结果永远覆盖不了新输入」。
///
/// 翻页是**惰性**的：命中强触发只把 `completed` 置位，句子文本留在原地，
/// 让壳来得及为这句完结的文本发请求；下一次喂进中文时才把旧句挪进 `previous`。
#[derive(Debug, Default, Clone)]
pub struct SentenceContext {
    sentence_id: u64,
    version: u64,
    text: String,
    completed: bool,

    /// 上一句完结时的内容：给模型当上下文，不发给别处。
    previous: Option<String>,
}

/// 强触发的句末标点：见到任何一个就当这句话说完了。
const SENTENCE_ENDINGS: [char; 4] = ['。', '！', '？', '；'];

impl SentenceContext {
    /// 喂进一段刚上屏的文本（候选、整句、标点……）。
    ///
    /// 英文直输、恢复出来的半角 `?` 这类不含中文的上屏不值得花一次请求，直接忽略；
    /// 文本里出现句末标点就把这句子标记完结（惰性翻页，见结构体文档）。
    pub fn feed(&mut self, text: &str) -> Feed {
        let text = text.trim();
        if !has_cjk(text) {
            return Feed {
                changed: false,
                completed: false,
            };
        }
        if self.completed {
            self.roll();
        }
        self.version += 1;
        self.text.push_str(text);
        self.completed = is_sentence_end(&self.text);
        Feed {
            changed: true,
            completed: self.completed,
        }
    }

    /// 句子被打断（换行交给应用、切换输入源、删词重打）：当前内容当完结处理。
    /// 没有内容、或本来就完结着，什么都不发生。
    pub fn break_sentence(&mut self) -> Feed {
        if self.text.is_empty() || self.completed {
            return Feed {
                changed: false,
                completed: false,
            };
        }
        self.version += 1;
        self.completed = true;
        Feed {
            changed: true,
            completed: true,
        }
    }

    /// 组句外按了一次退格：把最后一个字符从句子里删掉。删空了就当没这回事。
    pub fn backspace(&mut self) {
        if self.text.pop().is_some() {
            self.version += 1;
        }
    }

    /// 整段作废（切焦点、选中删除等）：清空文本与上一句，sentence_id 递增，
    /// 保证后续新句子的 (sentence_id, version) 不会和作废前的旧请求撞上。
    pub fn clear(&mut self) {
        self.sentence_id += 1;
        self.version += 1;
        self.text.clear();
        self.completed = false;
        self.previous = None;
    }

    /// 当前句子的文本。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 上一句的文本（给模型当上下文）。
    pub fn previous(&self) -> Option<&str> {
        self.previous.as_deref()
    }

    pub fn sentence_id(&self) -> u64 {
        self.sentence_id
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn is_completed(&self) -> bool {
        self.completed
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// 上一句退位，开新句：序号 +1，版本号继续往前走（版本只要求单调，不要求每句从 1 开始）。
    fn roll(&mut self) {
        let finished = std::mem::take(&mut self.text);
        self.previous = Some(finished);
        self.sentence_id += 1;
        self.completed = false;
    }
}

/// 文本里有没有中日韩表意文字：英文直输、数字、半角标点都不值得送给 AI。
fn has_cjk(text: &str) -> bool {
    text.chars().any(is_cjk)
}

/// 句子是否完结：句末标点，或结尾的空括号 `（）`（用户手动标记打完了）。
fn is_sentence_end(text: &str) -> bool {
    text.chars().any(|c| SENTENCE_ENDINGS.contains(&c)) || text.ends_with("（）")
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3007 | 0x3001..=0x301F // CJK 符号与标点（。、《》……）
        | 0x3400..=0x4DBF | 0x4E00..=0x9FFF // CJK 统一表意文字
        | 0xF900..=0xFAFF | 0x20000..=0x2FA1F // 兼容与扩展区
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_only_text_is_ignored() {
        let mut context = SentenceContext::default();
        let feed = context.feed("hello");
        assert!(!feed.changed);
        assert!(context.is_empty());
    }

    #[test]
    fn incremental_feeds_accumulate_one_sentence() {
        let mut context = SentenceContext::default();
        assert!(context.feed("我").changed);
        assert!(context.feed("稍后").changed);
        assert!(context.feed("回复").changed);
        let last = context.feed("你");
        assert!(last.changed);
        assert!(!last.completed);
        assert_eq!(context.text(), "我稍后回复你");
        assert_eq!(context.sentence_id(), 0);
    }

    #[test]
    fn sentence_final_punctuation_completes_without_rolling() {
        let mut context = SentenceContext::default();
        context.feed("我稍后回复你");
        let feed = context.feed("。");
        assert!(feed.completed);
        // 惰性翻页：完结的句子留在原地，壳还能拿它发请求
        assert_eq!(context.text(), "我稍后回复你。");
        assert_eq!(context.sentence_id(), 0);
        assert!(context.is_completed());
    }

    #[test]
    fn next_sentence_rolls_and_keeps_previous() {
        let mut context = SentenceContext::default();
        context.feed("我稍后回复你。");
        assert!(context.feed("我们").changed);
        assert_eq!(context.text(), "我们");
        assert_eq!(context.previous(), Some("我稍后回复你。"));
        assert_eq!(context.sentence_id(), 1);
        assert!(!context.is_completed());
    }

    #[test]
    fn break_sentence_finishes_without_punctuation() {
        let mut context = SentenceContext::default();
        context.feed("我稍后回复你");
        let feed = context.break_sentence();
        assert!(feed.completed);
        assert_eq!(context.text(), "我稍后回复你");
        assert!(context.is_completed());
        // 空上下文再打断无事发生
        assert!(!context.break_sentence().changed);
    }

    #[test]
    fn backspace_trims_and_bumps_version() {
        let mut context = SentenceContext::default();
        context.feed("我们");
        let version = context.version();
        context.backspace();
        assert_eq!(context.text(), "我");
        assert_eq!(context.version(), version + 1);
        context.backspace();
        context.backspace(); // 删空后继续退格不再涨版本
        assert!(context.is_empty());
        assert_eq!(context.version(), version + 2);
    }

    #[test]
    fn cjk_punctuation_alone_starts_a_sentence() {
        // 用户单独上屏一个「。」也应被当成中文文本处理
        let mut context = SentenceContext::default();
        let feed = context.feed("。");
        assert!(feed.changed);
    }
}
