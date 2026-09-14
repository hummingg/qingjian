use serde_json::Value;

use crate::provider::{EnglishCoachRequest, EnglishCoachResponse, LearningPhrase};

/// 系统提示：任务不是「翻译」，而是「给出母语者在这个语境下最自然会说的话」。
pub fn system_prompt() -> &'static str {
    "You are an English coach embedded in a Chinese input method. \
The user types Chinese; you reply with the English a native speaker would naturally say \
in the same situation — NOT a literal translation.

Rules:
1. Prefer the most natural, idiomatic expression over word-for-word accuracy.
2. Preserve the original meaning and tone.
3. Use the previous sentence only to resolve context; never translate it.
4. Match the learner's CEFR level; avoid rare or overly complex wording.
5. Pick 1-2 phrases worth learning (multi-word expressions, collocations, phrasal verbs). \
Skip the sentence if it is trivial (a greeting, a single common word).

Reply with a single JSON object and nothing else:
{\"english\": \"...\", \"phrases\": [{\"phrase\": \"...\", \"meaning\": \"中文释义\", \"cefr\": \"B1\", \"example\": \"...\"}]}"
}

/// 用户消息：水平、前文（受控长度）、当前句子。
pub fn user_prompt(request: &EnglishCoachRequest) -> String {
    let mut prompt = format!("Learner CEFR level: {}.\n", request.learner_level);
    if let Some(previous) = request.previous.non_empty_trimmed() {
        prompt.push_str(&format!("Previous sentence (context only): {previous}\n"));
    }
    prompt.push_str(&format!(
        "Sentence to express in natural English: {}",
        request.text
    ));
    prompt
}

/// 解析模型回复：取第一个 JSON 对象，抽 `english` 与 `phrases`。
///
/// 模型偶尔不听话（套 markdown 代码块、前后带说明），所以扫整个正文找第一个 `{` 到最后一个 `}`。
pub fn parse_reply(content: &str, max_phrases: usize) -> Result<EnglishCoachResponse, String> {
    let trimmed = content.trim();
    let Some(json) = extract_json_object(trimmed) else {
        return Err(format!("回复里没有 JSON 对象：{trimmed}"));
    };
    let value: Value = serde_json::from_str(json).map_err(|error| error.to_string())?;
    let english = value
        .get("english")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "回复缺少 english 字段".to_owned())?
        .to_owned();
    let phrases = value
        .get("phrases")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let phrase = item.get("phrase")?.as_str()?.trim();
                    if phrase.is_empty() {
                        return None;
                    }
                    Some(LearningPhrase {
                        phrase: phrase.to_owned(),
                        meaning: item
                            .get("meaning")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .trim()
                            .to_owned(),
                        cefr: item
                            .get("cefr")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .trim()
                            .to_owned(),
                        example: item
                            .get("example")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .trim()
                            .to_owned(),
                    })
                })
                .take(max_phrases)
                .collect()
        })
        .unwrap_or_default();
    Ok(EnglishCoachResponse { english, phrases })
}

/// 取正文里第一个 `{` 到与之配对的 `}` 之间的内容；找不到返回 `None`。
fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, byte) in bytes.iter().enumerate().skip(start) {
        match *byte {
            b'\\' if in_string => escaped = !escaped,
            b'"' if !escaped => in_string = !in_string,
            b'{' if !in_string => depth += 1,
            b'}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return text.get(start..=index);
                }
            }
            _ => {}
        }
        if *byte != b'\\' {
            escaped = false;
        }
    }
    None
}

trait NonEmptyTrimmed {
    fn non_empty_trimmed(&self) -> Option<&str>;
}

impl NonEmptyTrimmed for Option<String> {
    fn non_empty_trimmed(&self) -> Option<&str> {
        self.as_deref().map(str::trim).filter(|s| !s.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_json_reply() {
        let reply = r#"{"english": "I'll get back to you later.", "phrases": [{"phrase": "get back to somebody", "meaning": "回复某人", "cefr": "B1", "example": "I'll get back to you tomorrow."}]}"#;
        let response = parse_reply(reply, 2).unwrap();
        assert_eq!(response.english, "I'll get back to you later.");
        assert_eq!(response.phrases.len(), 1);
        assert_eq!(response.phrases[0].phrase, "get back to somebody");
        assert_eq!(response.phrases[0].cefr, "B1");
    }

    #[test]
    fn parses_reply_wrapped_in_markdown_fence() {
        let reply = "```json\n{\"english\": \"This approach isn't feasible for now.\", \"phrases\": []}\n```";
        let response = parse_reply(reply, 2).unwrap();
        assert_eq!(response.english, "This approach isn't feasible for now.");
        assert!(response.phrases.is_empty());
    }

    #[test]
    fn caps_phrases_at_the_requested_count() {
        let reply = r#"{"english": "ok", "phrases": [
            {"phrase": "a", "meaning": "甲"}, {"phrase": "b", "meaning": "乙"}, {"phrase": "c", "meaning": "丙"}]}"#;
        let response = parse_reply(reply, 2).unwrap();
        assert_eq!(response.phrases.len(), 2);
    }

    #[test]
    fn rejects_reply_without_english() {
        assert!(parse_reply("{\"phrases\": []}", 2).is_err());
        assert!(parse_reply("不是 JSON", 2).is_err());
    }

    #[test]
    fn extract_json_object_handles_braces_inside_strings() {
        let text = r#"前言 {"english": "a {b} c", "phrases": []} 后语"#;
        assert_eq!(
            extract_json_object(text),
            Some(r#"{"english": "a {b} c", "phrases": []}"#)
        );
    }
}
