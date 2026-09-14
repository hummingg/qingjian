# Qingjian English Coach 技术设计文档

项目定位： 基于 Qingjian（青简）二次开发的 AI 英语学习型 macOS 中文输入法。
第一阶段目标： 保留 Qingjian 原有中文输入法能力，在其基础上增加 AI English Coach，使用户每次输入中文时，都能持续看到对应的自然英文表达，在不打断输入的情况下实现被动英语学习。

---

## 1. 产品定位

```
传统输入法：
拼音 → 中文

Qingjian：
拼音 → 中文
       ↓
     英文词义

Qingjian English Coach：
中文输入
   ↓
AI 理解上下文
   ↓
自然英文表达
   ↓
持续展示
   ↓
被动学习
   ↓
主动复习 / SRS
```

核心理念：

> Every sentence you type is an opportunity to learn English.

中文输入不是学习软件中的“练习题”，而是用户本来就要完成的工作。

---

## 2. 核心产品原则

### 2.1 英文句子始终显示

这是本项目最重要的 UI 原则。

用户输入中文时，English Coach 默认处于工作状态。

不能设计成：

```
点击“翻译”
↓
才显示英语
```

而应该：

```
用户正常输入中文
        ↓
英文 Coach 自动显示
```

例如：

```
我稍后回复你

🇬🇧 I'll get back to you later.
```

用户不需要任何额外操作。

### 2.2 中文输入优先

AI 绝对不能阻塞中文输入。

```
正确流程：

用户输入
    │
    ├──→ Qingjian 中文输入引擎
    │          ↓
    │       立即上屏
    │
    └──→ English Coach
               ↓
            异步 AI
               ↓
          更新英文 UI
```

AI 出问题时：

- 中文输入：正常
- 英文 Coach：暂时不可用

不能因为 AI 网络超时导致输入法卡顿。

### 2.3 英文默认简洁

正常状态只显示：

```
🇬🇧 I'll get back to you later.
```

不要默认显示大量教学内容。

用户主动查看时，再显示：

```
get back to somebody
= 回复某人 / 回头联系某人

CEFR: B1
```

这样同时满足：

- 被动学习
- 低干扰
- 不打断工作

---

## 3. 用户交互

### 3.1 逐词输入

用户输入：

```
我
然后：
我稍后
然后：
我稍后回复
然后：
我稍后回复你
```

English Coach 必须维护当前中文上下文。

最终：

```
我稍后回复你

🇬🇧 I'll get back to you later.
```

因此：

逐词上屏不等于只能逐词翻译。
输入法内部必须维护一个 SentenceContext。

---

## 4. SentenceContext

新增核心组件：

> SentenceContext

职责：

1. 记录当前句子的中文内容
2. 识别句子边界
3. 处理用户修改
4. 生成 AI 请求上下文
5. 防止旧 AI 结果覆盖新输入

建议结构：

```rust
struct SentenceContext {
    sentence_id: String,
    version: u64,
    text: String,
    started_at: DateTime,
    updated_at: DateTime,
    completed: bool,
}
```

其中：

- `sentence_id` 用于标识一句话。
- `version` 用于处理异步 AI 返回。

例如：

```
version = 4
AI 正在处理。

用户又修改了文本：
version = 5

此时 version 4 的 AI 结果返回：
response.version != current.version
直接丢弃。
```

禁止旧结果覆盖新结果。

---

## 5. 句子识别

不能每输入一个字都调用 AI。
需要 Sentence Detector + Debounce。

触发条件：

强触发

- 。
- ！
- ？
- ；
- 回车
- 发送

弱触发

- 用户停止输入：500ms ~ 1500ms

具体 debounce 时间应该通过配置或实验确定。

---

## 6. AI 请求策略

第一版采用异步请求。

```
中文输入
   ↓
SentenceContext 更新
   ↓
Debounce
   ↓
AI Request
   ↓
AI Response
   ↓
校验 sentence_id + version
   ↓
更新 UI
```

不要：

```
输入一个字
↓
请求 AI
↓
等待
↓
才能继续输入
```

---

## 7. AI Provider 抽象

不要把项目绑定到某一个 AI 厂商。

定义统一接口：

```rust
trait EnglishCoachProvider {
    async fn generate(
        &self,
        request: EnglishCoachRequest,
    ) -> Result<EnglishCoachResponse>;
}
```

请求：

```rust
struct EnglishCoachRequest {
    text: String,
    context: Option<String>,
    learner_level: Option<String>,
    sentence_id: String,
    version: u64,
}
```

返回：

```rust
struct EnglishCoachResponse {
    sentence_id: String,
    version: u64,
    english: String,
    phrases: Vec<LearningPhrase>,
}
```

---

## 8. AI Provider 第一阶段

至少设计：

> OpenAI-compatible Provider

这样可以兼容大量 API。

配置：

- Provider
- Base URL
- API Key
- Model

同时预留：

- Ollama
- OpenAI
- Gemini
- Anthropic
- Custom OpenAI-compatible API

---

## 9. AI 默认策略

AI 不应该默认拥有无限上下文。

第一阶段：

```
当前中文句子
+
必要的前一句上下文
```

例如：

```
上一句：
这个方案我已经看过了。

当前句：
我们明天再讨论。
```

AI 可以理解为：

> Let's discuss it tomorrow.

而不是机械翻译：

> Let's discuss this tomorrow.

上下文长度应该控制，避免：

- Token 浪费
- 延迟增加
- 隐私风险

---

## 10. AI Prompt 原则

AI 的任务不是：

> Translate Chinese into English.

而是：

> Produce the most natural English expression a native speaker would use in this context.

核心要求：

1. 优先自然表达
2. 不逐字翻译
3. 保持原意
4. 根据上下文选择表达
5. 避免过度复杂
6. 适合当前用户英语水平
7. 识别值得学习的短语

例如：

中文：

> 这个方案暂时不可行。

不要固定返回：

> This plan is temporarily infeasible.

更自然：

> This approach isn't feasible for now.

---

## 11. English Coach UI

UI 分为三层。

第一层：始终显示

```
🇬🇧 I'll get back to you later.
```

这是默认 UI。

第二层：学习信息

用户点击/悬停：

```
get back to somebody

回复某人 / 回头联系某人

CEFR B1
```

第三层：主动学习

用户选择学习：

```
怎么用英语说？

我稍后回复你

I'll ______ back to you later.

[ get ]
```

---

## 12. UI 原则

必须避免：

- 弹窗
- 强制答题
- 遮挡输入
- 频繁动画
- 不断闪烁

English Coach 是：

> 输入法中的一层辅助信息

而不是：

> 突然出现的英语学习软件

---

## 13. AI 结果更新策略

AI 可以后台持续更新，但 UI 不应该频繁跳变。

例如：

```
我稍后
AI 初步：
I'll do it later.

继续输入：
我稍后回复你
AI：
I'll get back to you later.
```

只在新结果明显更完整时更新。

需要考虑结果稳定性。

可以采用：

- minimum confidence
- debounce
- version check

避免 UI 闪烁。

---

## 14. LearningPhrase

AI 返回值得学习的表达：

```rust
struct LearningPhrase {
    phrase: String,
    meaning: String,
    cefr: Option<String>,
    example: Option<String>,
}
```

例如：

```
phrase:
get back to somebody

meaning:
回复某人 / 回头联系某人

cefr:
B1

example:
I'll get back to you tomorrow.
```

---

## 15. 学习数据

AI 负责生成内容。
本地负责记录学习状态。

不要把“完整英语表达数据库”作为第一阶段核心。

本地数据库主要保存：

- 用户看过什么
- 用户使用过什么
- 用户答对多少次
- 什么时候复习
- 掌握程度

而不是试图预先覆盖所有中文表达。

---

## 16. SRS

新增：

> English Coach SRS

记录：

- phrase_id
- seen_count
- attempt_count
- correct_count
- last_seen_at
- next_review_at
- interval
- ease_factor
- mastery

基本流程：

```
AI生成表达
      ↓
用户看到
      ↓
记录 exposure
      ↓
用户再次遇到
      ↓
主动回忆
      ↓
正确
      ↓
延长复习间隔
```

---

## 17. 被动学习和主动学习必须分离

被动学习：

- 默认开启
- 无操作
- 始终显示英文

主动学习：

- 用户主动进入
- SRS
- 填空
- 复习
- 错题

不要强迫用户学习。

---

## 18. 用户个性化

后续可以记录：

用户经常输入：“稍后回复”

AI 可能发现用户反复遇到：get back to

于是提高学习权重。

用户已经非常熟悉：get back to

则逐渐减少提示。

形成：

```
输入
 ↓
AI
 ↓
学习记录
 ↓
个性化
 ↓
下一次 AI
```

---

## 19. 用户英语水平

支持：

- A1
- A2
- B1
- B2
- C1
- C2

例如 B1 用户：

```
这个方案暂时不可行。

This approach isn't feasible for now.
```

而不是主动给：

> This proposition is presently impracticable.

AI 应根据用户水平控制表达复杂度。

---

## 20. 场景理解

后续支持：

- 日常聊天
- 工作
- GitHub
- 编程
- 邮件
- 旅行
- 学习
- 社交媒体

同一个中文：

> 这个问题我们稍后再讨论。

工作场景：

> Let's discuss this issue later.

聊天：

> Let's talk about this later.

GitHub：

> Let's discuss this issue in more detail later.

因此 AI Coach 的核心价值不是翻译，而是：

> Context-aware English expression

---

## 21. 与 Qingjian 的关系

原则：

尽量不修改 Qingjian 已经成熟的输入核心。

保留：

- Pinyin
- 候选
- 模糊音
- 双拼
- 词频
- 个人学习
- 中文语言模型
- macOS InputMethodKit

新增：

- SentenceContext
- AI Provider
- English Coach
- LearningPhrase
- SRS

整体：

```
┌─────────────────────────────────┐
│       macOS InputMethodKit      │
├─────────────────────────────────┤
│          Qingjian Core          │
│                                 │
│ Pinyin → Candidate → Selection  │
├─────────────────────────────────┤
│        English Coach Layer      │
│                                 │
│ SentenceContext                 │
│        ↓                        │
│ AI Provider                     │
│        ↓                        │
│ English Expression              │
│        ↓                        │
│ LearningPhrase                  │
│        ↓                        │
│ SRS                             │
├─────────────────────────────────┤
│        Local Learning DB        │
└─────────────────────────────────┘
```

---

## 22. 数据流

完整数据流：

```
用户键盘
   ↓
macOS IMK
   ↓
Qingjian Core
   ↓
中文候选
   ↓
用户选择
   ↓
上屏
   ↓
SentenceContext
   ↓
Debounce
   ↓
EnglishCoach
   ↓
AI Provider
   ↓
EnglishCoachResponse
   ↓
version 校验
   ↓
English Coach UI
   ↓
用户看到英文
   ↓
Learning DB
   ↓
SRS
```

---

## 23. AI 失败处理

任何以下情况：

- 网络错误
- 超时
- API Key 错误
- 模型错误
- 返回格式错误
- 限流

均不得影响：

> 中文输入

UI 可以显示：

> 🇬🇧 English Coach unavailable

或者直接保留上一次结果。

---

## 24. 隐私

第一阶段遵循：

> Local-first, AI optional.

默认情况下：

```
中文输入 → 本地处理
```

启用 AI 后：

```
当前句子 → 用户指定 AI Provider
```

不得默认上传：

- 完整输入历史
- 全部输入内容
- 用户词库
- 学习记录

除非用户明确开启相关功能。

---

## 25. 性能目标

中文输入链路：AI 不参与。

因此目标：

- 中文候选延迟：保持 Qingjian 原有性能
- AI：完全异步
- English Coach 延迟可以接受：约 0.5 ~ 数秒

但不能影响中文输入。

---

## 26. 第一阶段 MVP

只实现：

P0

1. Fork Qingjian
2. macOS
3. 保留原有中文输入
4. SentenceContext
5. AI Provider
6. AI 生成整句英文
7. 英文 UI 始终显示
8. 异步请求
9. version 防旧结果
10. AI 配置页面

P1

11. 学习短语
12. CEFR
13. 点击查看解释
14. 本地学习记录
15. SRS

P2

16. 场景
17. 用户英语水平
18. 个性化
19. Ollama
20. 多 AI Provider

---

## 27. 第一阶段明确不做

不要一开始做：

- ❌ Windows
- ❌ Linux
- ❌ iOS
- ❌ Android
- ❌ 完整英语课程
- ❌ 海量本地表达数据库
- ❌ 社区
- ❌ 账号系统
- ❌ 云端同步
- ❌ 复杂 Gamification

先把：

> macOS 中文输入 + 始终显示 AI 英语

做到非常好。

---

## 28. Git 分支策略

建议：

```
upstream:
qingjian-team/qingjian

fork:
qingjian-english-coach
```

分支：

```
main
│
├── feature/sentence-context
├── feature/ai-provider
├── feature/english-coach-ui
├── feature/learning-phrase
└── feature/srs
```

尽量保持 Qingjian 上游可同步。

---

## 29. 开源协议

Qingjian 当前代码采用：

> GPL-3.0-or-later

二次开发必须遵守 GPL 要求。

同时必须单独检查：

- Qingjian 第三方数据
  - 词典
  - 语言模型
  - CEFR 数据
- 依赖库
- AI SDK

的各自许可证。

尤其注意：

代码采用 GPL，并不意味着所有附带数据自动变成 GPL。
“Qingjian / 青简”名称和 Logo 也不能因为代码 GPL 就自动获得使用权。

因此项目正式发布前需要重新确认品牌名称和所有第三方数据授权。

---

## 30. 推荐项目名称

开发阶段：

> Qingjian English Coach

仓库可以：

> qingjian-english-coach

最终产品名称可以以后再决定。

第一阶段不要为了品牌重构代码。

---

## 31. Coding Agent 实施顺序

建议 Coding Agent 严格按照以下顺序执行：

```
Step 1  阅读 Qingjian 当前源码结构
Step 2  找到 macOS InputMethodKit shell
Step 3  找到中文候选/上屏生命周期
Step 4  找到现有 translation / learning 数据流
Step 5  实现 SentenceContext
Step 6  实现 AI Provider trait
Step 7  实现 OpenAI-compatible Provider
Step 8  实现异步 AI 请求
Step 9  实现 version 防竞态
Step 10 实现 English Coach UI
Step 11 实现 LearningPhrase
Step 12 实现本地学习记录
Step 13 实现 SRS
Step 14 完善设置和错误处理
Step 15 macOS 全面测试
```

---

## 32. 最重要的验收标准

### 输入性能

断网：

> 中文输入仍然正常

AI 超时：

> 中文输入仍然正常

API 崩溃：

> 中文输入仍然正常

### 英语显示

输入：

> 我稍后回复你

最终必须出现：

> I'll get back to you later.

而不是：

> I later reply you.

### 逐词输入

用户逐词上屏：

```
我
我稍后
我稍后回复
我稍后回复你
```

最终仍然能够生成：

> I'll get back to you later.

### 修改文本

旧文本：

```
我稍后回复你
AI：
I'll get back to you later.
```

用户修改：

```
我回复你
```

旧 AI 结果不得覆盖新结果。

### 被动学习

用户无需点击任何按钮：

```
输入中文
↓
看到英文
```

必须成立。

---

## 33. 最终产品体验

理想状态下，用户甚至不会感觉自己在“学英语”。

每天正常使用电脑：

- 写微信
- 写邮件
- 写代码
- 写 GitHub issue
- 写文档
- 聊天

旁边始终有：

> 🇬🇧 Natural English

一个月以后：

- get back to
- figure out
- make sense
- work on
- be supposed to
- in terms of
- as long as

这些表达开始变得熟悉。

之后：

```
被动看到
    ↓
主动使用
    ↓
主动回忆
    ↓
SRS
    ↓
真正掌握
```

最终产品不是：

> 一个带 AI 翻译的输入法

而是：

> 一个把日常中文输入转换成持续英语暴露和学习机会的输入法。

这就是 Qingjian English Coach 的核心差异化。
