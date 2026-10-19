# DeepSeek Android 端「流式输出自动滚动」逆向笔记

目标包：`com.deepseek.chat` 2.6.1 (versionCode 280)，从手机 `adb pull` 出 base + arm64 split，
jadx 1.5.1 反编译，jadx 反编译失败的关键协程体用 `dexdump -d` 还原。

结论先说：**它的顺滑不是靠某个神奇的滚动 API，而是一套「跟随状态机 + 50ms 合并 tick + 低刚度弹簧 + 逐帧 scrollBy」的实现，
再加一条很关键的交互规则：回答超过一屏就停止跟随、把回答顶部钉在视口顶部。**

---

## 1. 怎么定位到混淆后的代码

APK 用 R8 全量混淆（类名全进 default package，jadx 里显示成 `defpackage.g32` 之类），
但 Compose 编译器生成的 trace 字符串把**原始文件/函数名/行号**保留了下来：

```
z43.f("com.deepseek.chat.pages.chat.StreamingSmoothAutoScrollOneScreenEffect (ChatMessageListUIEffect.kt:162)")
```

于是可以用 `grep "ChatMessageListUIEffect.kt"` 反查混淆类。映射结果：

| 原始位置 | 混淆类 | 作用 |
| --- | --- | --- |
| `ChatMessageListUIEffect.kt:46`  `BottomPaddingChangeEffect` | `g32.a` / `y22` | 输入框/键盘导致底部 padding 变化时补滚动 |
| `ChatMessageListUIEffect.kt:76`  `StreamingAutoScrollEffect` | `g32.c` / `a32` | 旧版「跳一下」跟随（`scrollBy`，无动画） |
| `ChatMessageListUIEffect.kt:162` `StreamingSmoothAutoScrollOneScreenEffect` | `g32.d` / `e32` / `d32` | 新版「平滑跟随 + 一屏暂停」 |
| `ChatMessageListUIEffect.kt:499` `StreamingSmoothScrollEffect` | `g32.e` / `f32` | 另一处平滑滚动（非主链路） |
| `ChatMessageListUIEffect.kt:561` `NewMessageEventsEffect` | `g32.b` / `ns1` | 新消息事件触发的滚动 |
| `ChatMessageList.kt:78` `ChatMessageList` | `w22` | 列表本体 + `scrollToBottom` 工具函数 |
| `AnimatedLazyList.kt:358` `AnimatedLazyColumn` | `ib3.a` | 包了一层 item 增减动画的 LazyColumn |
| `ChatFloatingToolViewV2.kt:229` `GotoBottomButton` | — | 右侧那个「回到底部」的圆角下箭头按钮 |

关键类型（都是 Compose 原类型被 R8 改名）：

| 混淆名 | 真实类型 |
| --- | --- |
| `fi6` | `LazyListState` |
| `oh6` / `jh6` | `LazyListLayoutInfo` / `LazyListItemInfo` |
| `pj` | `Animatable<Float, AnimationVector1D>` |
| `cd9.N` | `ScrollableState.scrollBy(value)`（suspend） |
| `fi6.l(index, offset)` | `LazyListState.scrollToItem(index, offset)` |
| `fi6.f(spec, block)` | `LazyListState.scroll(block)`（占住 scroll mutex 的协程块） |
| `w22.c(state)` | `state.scrollToItem(Int.MAX_VALUE, 0)` —— 精确「到最底」 |
| `w22.d(state)` | 找到最后一个高度 > 0 的可见 item，`scrollToItem(it.index, it.size)` —— 靠 clamp 落到「最底」 |
| `x0a` | `Scroll(distance: Float, snapToResponseTop: Boolean)` |

---

## 2. 五个 effect 里真正起作用的三段逻辑

### 2.1 底部 padding 变化（`BottomPaddingChangeEffect`）

```kotlin
LaunchedEffect(bottomPaddingPx) {
    val delta = bottomPaddingPx - previousPaddingPx
    if (delta > 0 && isFollowing &&
        !listState.isScrollInProgress &&
        currentStreamingMessageIndex == capturedIndex        // 还是同一条回答
    ) {
        listState.scrollToItem(Int.MAX_VALUE, 0)             // 直接贴底，不做动画
    }
    previousPaddingPx = bottomPaddingPx
}
```

要点：

* 只在 padding **变大**时补，不变小时不补（键盘收起不会把内容顶走）。
* 用 `scrollToItem(Int.MAX_VALUE, 0)` 而不是 `scrollBy(delta)`：后者在列表已经到底时会把
  overscroll 当成位移，容易越界/回弹；前者让 LazyColumn 自己 clamp，数学上永远正确。
* 还要求「scroll 不在进行中」且「流式回答没换人」。

### 2.2 旧版跟随（`StreamingAutoScrollEffect`，`sse_smooth_follow=false` 时用）

每次流式内容变化，重新算「离底部还差多少」，然后 `scrollBy(差值)` 一次性跳过去：

```kotlin
LaunchedEffect(streamingIndex) {
    while (true) {
        if (!isFollowing || isDragged) return
        if (msgIndex != streamingIndex) return
        val distance = distanceToBottom()          // 见 2.4
        if (distance > 0f) {
            if (bottomGap >= distance && something) snapToBottom()
            scrollBy(distance)                     // 无动画
        } else {
            isFollowing = false
        }
        delay(?)                                    // 等下一次内容变化
    }
}
```

### 2.3 新版平滑跟随 + 一屏暂停（`StreamingSmoothAutoScrollOneScreenEffect`，默认开启）

这才是「体验好」的来源。

```kotlin
LaunchedEffect(streamingIndex) {
    val anim = Animatable(0f)

    if (!isActive) return@LaunchedEffect
    if (!followEnabled || dragging || listState.isScrollInProgress) return@LaunchedEffect

    listState.scroll {                                  // 占住 scroll mutex（用户拖动优先级更高，会抢占）
        while (isActive) {
            if (!isFollowing || dragging) break
            if (listState.isScrollInProgress) break
            if (msgIndex != streamingIndex) break        // 换成下一条回答

            when (val target = computeScrollTarget(...)) {   // 见 2.4
                Scroll -> {
                    val start = anim.value
                    anim.animateTo(
                        targetValue = target.distance,
                        animationSpec = spring(
                            // 关键：snapToResponseTop 时用 200，否则用远端配置（默认 50）
                            stiffness = if (target.snapToResponseTop) 200f else configuredStiffness,
                            dampingRatio = 1f,              // 无回弹
                        ),
                    ) {
                        val delta = value - start
                        val consumed = scrollBy(delta)      // 逐帧把弹簧位移喂给列表
                        start = value
                        if (abs(delta - consumed) > 0.5f) { // 有一帧滚不动了（到边界/被抢）
                            cancelAnimation()               // 立刻放弃，别累积误差
                        }
                    }
                }
                StopFollowing -> isFollowing = false
                NoScroll -> break
            }
            delay(50)                                      // 合并 token 风暴
        }
    }
}
```

要点（体验差异的真正来源）：

1. **50ms tick 合并 token**：SSE 一秒可能来几十个 delta，逐 delta 触发滚动会抖。
   每 50ms 重新读一次布局、算一次目标，把一整段时间的内容增长合并成一次弹簧动画。
2. **低刚度弹簧（stiffness=50, dampingRatio=1）**：不是「滚到底」，而是「慢慢追上目标」。
   滚动动画还没结束，下一次 tick 又把目标推远，于是形成连续、略滞后于文字生长的顺滑跟随，
   而不是「跳一下、停一下」。
3. **逐帧 `scrollBy(弹簧增量)` 而不是 `animateScrollToItem`**：
   可以随时被用户手势抢占，也能在滚不动时通过 `delta - consumed > 0.5px` 立即取消，不会积压。
4. **整段循环包在 `listState.scroll { }` 里**：与用户拖动共享 mutex；
   用户一拖，`isScrollInProgress` 变 true，循环立刻 break。
5. **状态机**：`isFollowing` 一旦因为「用户上滑 / 一屏暂停 / 换消息」变 false，
   后续 tick 一律不滚，直到用户自己回到底部（或点右下角按钮）。

### 2.4 「离底部还有多远」怎么算

不是 `totalItemsCount`，而是布局数据：

```kotlin
fun distanceToBottom(): Float {
    val info = listState.layoutInfo
    val last = info.visibleItemsInfo.lastOrNull { it.size > 0 } ?: return 0f
    val viewportEnd = info.viewportEndOffset                // 已扣掉 contentPadding.end
    val contentBottom = last.offset + last.size
    return (contentBottom - viewportEnd).coerceAtLeast(0f)  // 还要往下滚多少
}

fun bottomGap(): Float = (viewportEnd - contentBottom).coerceAtLeast(0f)   // 距离底部还剩多少空白
```

此外每次流式内容变化时，消息 item 会把自己的测量尺寸写进一个
`CompositionLocal<SnapshotStateMap<Int, IntOffset>>`（key = 消息序号），
用来在 LazyColumn 还没完成 layout 结算时，按「内容真实尺寸」推算目标位置，
避免依赖尚未更新的 `layoutInfo`。

### 2.5 一屏规则：实测轨迹（不是猜的）

用 `adb screenrecord` 录下 DeepSeek 2.6.1 从发送到输出结束的完整过程，逐帧取 PTS 统计：

* 发一条 400 词的「灯塔守夜人」故事，录 31 秒，**只有 3 帧画面发生变化**（其余时间画面完全静止）；
* 最后稳定下来的状态：回答第一行贴着列表顶部，「正在思考 / 用户消息」已经滚出视口，
  内容继续在下方流出屏幕，右下角是一个「回到底部」的圆形下箭头；
* 也就是说：**它没有一路跟到底**。回答超过一屏后，它把回答顶部对齐到视口顶部，然后停住。

再录一次从发送瞬间开始的 7 秒（525 帧）：

| 时间 | 画面 |
| --- | --- |
| 0–2s | 用户消息在顶部，下面是「…」思考中，列表没动 |
| 3–4s | 「已思考（用时 1 秒）」出现，回答第一段开始流出 |
| 5s | 列表滚过一次，用户消息和思考块被顶出视口，**回答第一行落到视口顶部** |
| 6–7s | 文字继续增长，视口不再移动 |

截图：`docs/assets/deepseek-autoscroll/deepseek-one-screen-pause.png`（它停下来的样子）。

结论：

| 回答高度 | 行为 |
| --- | --- |
| `< 1 屏` | 跟随最新一行（平滑），保证新字可见 |
| `>= 1 屏` | 把回答顶部对齐视口顶部，**停住**，显示「回到底部」按钮 |

所以「顺滑」= 前半段的跟随动画 + 到点后干净地停在一个明确锚点上，而不是一直滚到底、
更不是停在半路。我们之前两版分别踩了这两个坑：一版停在半路，一版不设锚点。

### 2.6 远端可调参数（`kv_settings_*` 可被后台覆盖）

| 开关 | 含义 | 默认 |
| --- | --- | --- |
| `sse_smooth_follow` | 用平滑跟随还是旧的跳变跟随 | `true` |
| `sse_auto_scroll_one_screen` | 超过一屏暂停跟随 | `true` |
| `sse_auto_scroll_smooth_stiffness` | 弹簧刚度 | `50` |

---

## 3. 和 Hambur 现在的实现对比

Hambur 现在（`HamburChatScreen.kt` `ChatTimeline`）：

```kotlin
LaunchedEffect(displayItems.size) {
    if (displayItems.size > previousItemCount && isNearBottom && !isDragged) {
        listState.animateScrollToItem(displayItems.size)     // 一次性动画到底
    }
}
```

问题：

1. **触发源不对**：只监听 `displayItems.size`。流式输出时一个 markdown block 内部还在长字，
   高度变化完全不会触发滚动；等到下一个 block 出现才「跳」一次。
2. **用 `animateScrollToItem` 而不是「补差值」**：目标永远是最底部，
   动画期间到达的文字会被甩在后面，动画结束又是一次新的跳变，观感是「一顿一顿」。
3. **没有跟随状态机**：`isNearBottom` 是派生态，用户往上滑一点点就被判成「不跟随」，
   但没有任何地方把它恢复成「跟随」，也没有回到底部按钮。
4. **底部 padding 用 `scrollBy(delta)`**：列表已到底时会把 padding 当成可滚距离，容易多滚/回弹。
5. **没有一屏规则**：长回答会一直把用户拖到最新一行，想从开头读就得手动往回追。

---

## 4. 已经落到 Hambur 的改动

实现位置：`app/src/main/java/com/hambur/chat/ui/chat/HamburChatScreen.kt` 的 `ChatTimeline`。

1. **发送即收起键盘**（`onSend` 里 `clearFocus + hide`）。DeepSeek 发送后键盘会收起；
   我们之前键盘一直占着下半屏，回答只露出十几行，看起来就像「停在半路」。
2. **逐帧跟随**（`runChatFollowFrames`）：每帧重算「最后一行离视口底部还差多少」，
   用临界阻尼弹簧（ω=18）更新速度、逐帧 `scrollBy`，并对速度限幅 4500px/s。
   目标是移动的，所以是连续运动；输出结束时剩余距离平滑收敛到 0，一定滚到位。
   每 30 帧让出一次滚动锁，避免饿死 `scrollToItem` 这类一次性滚动。
3. **按内容真实高度判断一屏**：每个 display item 用 `onSizeChanged` 记录高度
   （`itemHeights`），累加出流式回答自己的高度，再和 `0.88 × 屏幕高度` 比较。
   不能拿当前可视区当基准——键盘弹出时可视区只有半屏，十几行就会误判。
4. **满一屏后对齐回答顶部**：`animateScrollToItem(streamingFirstIndex, 0)`，
   然后 `following = false`，右下角出现「回到最新」按钮（中英文案已加）。
5. **用户手势优先**：`isDragged` 立即停止跟随；手势结束回到底部则恢复；
   点按钮恢复跟随并 `animateScrollToItem(Int.MAX_VALUE, 0)`。

实测（Debug 包，装机验证）：

| 场景 | 结果 |
| --- | --- |
| 短回答（3 个词） | 无按钮，列表贴底，键盘自动收起 |
| 长回答（1..90 每行一个数字） | 高度到一屏时对齐回答顶部停住，显示下箭头；内容继续在下方流出 |
| 点下箭头 | 滚到 90 + 操作按钮行，按钮消失 |

截图：`docs/assets/deepseek-autoscroll/hambur-one-screen-pause.png`。

---

## 5. 复现命令（本机）

```bash
# 拉包
adb connect <phone>:5555
adb shell pm path com.deepseek.chat
adb pull /data/app/.../base.apk
# 反编译
HOME=$PWD/home JAVA_HOME=~/jdk-25 tools/bin/jadx --no-res -d out base.apk
# 关键文件（jadx 反编译失败时用 dexdump）
dexdump -d classes.dex > classes.dump
```

产物在 `.apk-inspect/`（已 gitignore）。
