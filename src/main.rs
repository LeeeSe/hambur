use anyhow::{Context, Result};
use dotenv::dotenv;
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use std::env;
use std::io::{self, Write};
use crossterm::{event::{poll, read, Event, KeyCode, KeyEvent}, 
                style::{Stylize, Color, SetForegroundColor, Print, ResetColor},
                terminal::{Clear, ClearType},
                cursor::{MoveUp, MoveToColumn},
                execute, queue};
use std::time::Duration;

mod models;
mod terminal;
use models::{ChatMessage, ChatRequest, ChatResponse, find_models, get_provider_by_model};
use terminal::RawModeGuard;

async fn send_chat_request(client: &reqwest::Client, message: &str, model_id: &str, message_history: &mut Vec<ChatMessage>) -> Result<String> {
    let start_time = tokio::time::Instant::now();
    let provider = get_provider_by_model(model_id)
        .context(format!("未找到模型 {} 的提供商", model_id))?;
    
    let api_key = env::var(&provider.api_key_env)
        .context(format!("未找到{}环境变量", provider.api_key_env))?;

    let mut headers = HeaderMap::new();
    
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", api_key))?,
    );
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

    message_history.push(ChatMessage {
        role: "user".to_string(),
        content: message.to_string(),
    });

    let request = ChatRequest {
        model: model_id.to_string(),
        messages: message_history.clone(),
        stream: true,
    };

    // 发送请求
    // 尝试发送请求，如果失败则直接输出错误信息
    if env::var("HAMBUR_DEBUG").is_ok() {
        eprintln!("[DEBUG] 请求准备耗时: {:?}", start_time.elapsed());
    }

    let request_start_time = tokio::time::Instant::now();
    let response = match client
        .post(&provider.api_base)
        .headers(headers)
        .json(&request)
        .send()
        .await {
            Ok(resp) => {
                // 检查HTTP状态码
                if resp.status().is_success() {
                    resp.bytes_stream()
                } else {
                    let status = resp.status();
                    let error_text = resp.text().await?;
                    
                    let error_msg = match status.as_u16() {
                        401 => format!("认证失败(401): API密钥可能无效或已过期。请检查{}环境变量设置。\n原始数据: {}", provider.api_key_env, error_text),
                        429 => format!("请求过多(429): 已超出API速率限制。\n原始数据: {}", error_text),
                        _ => format!("API请求失败({}): {}\n原始数据: {}", status.as_u16(), status.canonical_reason().unwrap_or("未知错误"), error_text)
                    };
                    
                    print!("{}", error_msg.clone().red());
                    io::stdout().flush()?;
                    return Ok(error_msg);
                }
            },
            Err(e) => {
                let error_msg = format!("API请求失败: {}\n请检查网络连接和API端点配置", e);
                print!("{}", error_msg.clone().red());
                io::stdout().flush()?;
                return Ok(error_msg);
            }
        };

    let mut full_response = String::new();
    
    io::stdout().flush()?;

    if env::var("HAMBUR_DEBUG").is_ok() {
        eprintln!("[DEBUG] 请求发送耗时: {:?}", request_start_time.elapsed());
    }

    let mut stream = response;
    let mut process_start_time = tokio::time::Instant::now();
    let mut total_chunks = 0;
    let mut total_chars = 0;
    let mut total_delay = tokio::time::Duration::from_secs(0);

    // 启用原始模式以捕获键盘事件
    let _raw_guard = RawModeGuard::enter()?;

    while let Some(chunk_result) = stream.next().await {
        // 检查是否有键盘事件
        if poll(Duration::from_millis(0))? {
            if let Event::Key(KeyEvent { code: KeyCode::Esc, .. }) = read()? {
                execute!(io::stdout(),
                    Print(format!("\n{}\n", "[已中断输出]".yellow()))
                )?;
                break;
            }
        }

        let chunk = chunk_result?;
        let chunk_str = String::from_utf8_lossy(&chunk);
        
        for line in chunk_str.lines() {
            // 跳过空行
            if line.trim().is_empty() {
                continue;
            }
            
            // 处理SSE格式的数据
            let data = if line.starts_with("data: ") {
                &line[6..]
            } else if line.starts_with(": OPENROUTER PROCESSING") {
                // 忽略 OpenRouter 的心跳消息
                continue;
            } else {
                // 如果不是标准SSE格式，尝试直接解析整行
                line
            };
            
            if data == "[DONE]" {
                continue;
            }
            
            // 尝试解析JSON响应
            match serde_json::from_str::<ChatResponse>(data) {
                Ok(response) => {
                    if let Some(choice) = response.choices.first() {
                        if let Some(reasoning) = &choice.delta.reasoning_content {
                            total_chars += reasoning.chars().count();
                            for c in reasoning.chars() {
                                let mut stdout = io::stdout();
                                if c == '\n' {
                                    // 换行时，先重置颜色，然后打印换行符，最后移动到行首
                                    queue!(stdout,
                                        ResetColor,
                                        Print("\n"),
                                        MoveToColumn(0)
                                    )?;
                                } else {
                                    queue!(stdout,
                                        SetForegroundColor(Color::Blue),
                                        Print(c.to_string()),
                                        ResetColor
                                    )?;
                                }
                                stdout.flush()?;
                                let delay = tokio::time::Duration::from_millis(10);
                                total_delay += delay;
                                tokio::time::sleep(delay).await;
                            }
                        }
                        
                        if let Some(content) = &choice.delta.content {
                            total_chunks += 1;
                            total_chars += content.chars().count();
                            for c in content.chars() {
                                let mut stdout = io::stdout();
                                if c == '\n' {
                                    // 换行时，先重置颜色，然后打印换行符，最后移动到行首
                                    queue!(stdout,
                                        ResetColor,
                                        Print("\n"),
                                        MoveToColumn(0)
                                    )?;
                                } else {
                                    queue!(stdout,
                                        SetForegroundColor(Color::Green),
                                        Print(c.to_string()),
                                        ResetColor
                                    )?;
                                }
                                stdout.flush()?;
                                let delay = tokio::time::Duration::from_millis(10);
                                total_delay += delay;
                                tokio::time::sleep(delay).await;
                            }
                            full_response.push_str(content);
                            
                            if env::var("HAMBUR_DEBUG").is_ok() {
                                eprintln!("[DEBUG] 处理{}个数据块耗时: {:?}", total_chunks, process_start_time.elapsed());
                                eprintln!("[DEBUG] 已处理{}个字符，累计输出延迟: {:?}", total_chars, total_delay);
                                process_start_time = tokio::time::Instant::now();
                            }
                        }
                    }
                },
                Err(e) => {
                    if env::var("HAMBUR_DEBUG").is_ok() {
                        eprintln!("[DEBUG] JSON解析错误: {}, 数据: {}", e, data);
                    }
                    
                    // 尝试其他可能的响应格式
                    if !data.starts_with('{') && !data.starts_with('[') {
                        // 如果不是JSON格式，直接显示文本内容
                        for c in data.chars() {
                            if c == '\n' {
                                // 换行时，先重置颜色，然后打印换行符，最后移动到行首
                                execute!(io::stdout(),
                                    ResetColor,
                                    Print("\n"),
                                    MoveToColumn(0)
                                )?;
                            } else {
                                execute!(io::stdout(),
                                    SetForegroundColor(Color::Green),
                                    Print(c.to_string()),
                                    ResetColor
                                )?;
                            }
                        }
                        full_response.push_str(data);
                    } else {
                        // 如果是JSON格式但解析失败，可能是错误响应，直接显示
                        let error_msg = format!("解析响应失败: {}\n原始数据: {}", e, data);
                        for c in error_msg.chars() {
                            if c == '\n' {
                                // 换行时，先重置颜色，然后打印换行符，最后移动到行首
                                execute!(io::stdout(),
                                    ResetColor,
                                    Print("\n"),
                                    MoveToColumn(0)
                                )?;
                            } else {
                                execute!(io::stdout(),
                                    SetForegroundColor(Color::Red),
                                    Print(c.to_string()),
                                    ResetColor
                                )?;
                            }
                        }
                        full_response.push_str(&error_msg);
                    }
                }
            }
        }
    }

    // 恢复终端模式会通过RawModeGuard的Drop实现自动处理
    
    println!();
    
    if env::var("HAMBUR_DEBUG").is_ok() {
        eprintln!("[DEBUG] 总耗时: {:?}", start_time.elapsed());
    }
    
    message_history.push(ChatMessage {
        role: "assistant".to_string(),
        content: full_response.clone(),
    });
    
    Ok(full_response)
}

async fn interactive_mode(client: &reqwest::Client) -> Result<()> {
    println!("{}", "欢迎使用Hambur，输入'exit'退出，'clear'清空聊天记录，直接输入模型关键字切换模型，连续按两次ESC退出程序".blue().bold());
    
    let mut message_history: Vec<ChatMessage> = Vec::new();
    let mut current_model = String::from("google/gemini-2.0-flash-001"); // 默认使用gemini-flash
    
    // 用于跟踪ESC按键
    let mut last_esc_time: Option<std::time::Instant> = None;
    
    'outer: loop {
        execute!(io::stdout(),
            MoveToColumn(0),
            Print(format!("{} ", "你:".cyan().bold()))
        )?;
        io::stdout().flush()?;
        
        // 启用原始模式以捕获键盘事件
        let _raw_guard = RawModeGuard::enter()?;
        
        let mut input = String::new();
        let mut reading = true;
        
        while reading {
            if poll(Duration::from_millis(100))? {
                match read()? {
                    Event::Key(KeyEvent { code: KeyCode::Esc, .. }) => {
                        // 检查是否是连续两次ESC
                        let now = std::time::Instant::now();
                        if let Some(last_time) = last_esc_time {
                            // 如果两次ESC按键间隔小于500毫秒，则退出程序
                            if now.duration_since(last_time).as_millis() < 500 {
                                execute!(io::stdout(),
                                    MoveToColumn(0),
                                    Print("\n[连续按两次ESC，程序已退出]\n"),
                                    ResetColor
                                )?;
                                return Ok(());
                            }
                        }
                        last_esc_time = Some(now);
                    },
                    Event::Key(KeyEvent { code: KeyCode::Enter, .. }) => {
                        execute!(io::stdout(), Print("\n"), MoveToColumn(0))?;
                        reading = false;
                    },
                    Event::Key(KeyEvent { code, .. }) => {
                        // 重置ESC计时器
                        last_esc_time = None;
                        
                        // 处理其他按键
                        match code {
                            KeyCode::Char(c) => {
                                input.push(c);
                                print!("{}", c);
                                io::stdout().flush()?;
                            },
                            KeyCode::Backspace => {
                                if !input.is_empty() {
                                    input.pop();
                                    // 删除一个字符（退格、空格、再退格）
                                    execute!(io::stdout(),
                                        Print("\u{8} \u{8}")
                                    )?;
                                    io::stdout().flush()?;
                                }
                            },
                            _ => {}
                        }
                    },
                    _ => {}
                }
            }
        }
        
        // 恢复终端模式会通过RawModeGuard的Drop实现自动处理
        
        let input = input.trim();
        if input.eq_ignore_ascii_case("exit") {
            break;
        } else if input.eq_ignore_ascii_case("clear") {
            // 清空聊天记录
            message_history.clear();
            execute!(io::stdout(),
                MoveToColumn(0),
                Print(format!("{}", "[聊天记录已清空]\n".yellow()))
            )?;
            continue;
        } else {
            // 先尝试查找匹配的模型
            let matches = find_models(input);
            
            if !matches.is_empty() {
                // 找到匹配的模型
                match matches.len() {
                    0 => {}, // 不可能发生，因为前面已经检查过matches不为空
                    1 => {
                        // 只有一个匹配，直接切换
                        let model = &matches[0];
                        current_model = model.id.clone();
                        execute!(io::stdout(),
                            MoveToColumn(0),
                            SetForegroundColor(Color::Green),
                            Print("已切换到模型: "),
                            Print(model.name.clone()),
                            Print("\n"),
                            ResetColor
                        )?;
                        continue;
                    },
                    _ => {
                        // 多个匹配，使用上下方向键选择
                        execute!(io::stdout(),
                            MoveToColumn(0),
                            Print(format!("{}", "找到多个匹配的模型，请使用上下方向键选择:\n".yellow()))
                        )?;
                        
                        // 启用原始模式以捕获键盘事件
                        let _raw_guard = RawModeGuard::enter()?;
                        
                        let mut selected_index = 0;
                        let mut selected = false;
                        
                        // 显示初始选择
                        for (i, model) in matches.iter().enumerate() {
                            if i == selected_index {
                                execute!(io::stdout(),
                                    MoveToColumn(0),
                                    Print(format!("{} {} ({})", ">".green(), model.name, model.provider)),
                                    Print("\n")
                                )?;
                            } else {
                                execute!(io::stdout(),
                                    MoveToColumn(0),
                                    Print(format!("  {} ({})", model.name, model.provider)),
                                    Print("\n")
                                )?;
                            }
                        }
                        
                        // 处理键盘事件
                        while !selected {
                            if poll(Duration::from_millis(100))? {
                                match read()? {
                                    Event::Key(KeyEvent { code: KeyCode::Up, .. }) => {
                                        if selected_index > 0 {
                                            selected_index -= 1;
                                        }
                                    },
                                    Event::Key(KeyEvent { code: KeyCode::Down, .. }) => {
                                        if selected_index < matches.len() - 1 {
                                            selected_index += 1;
                                        }
                                    },
                                    Event::Key(KeyEvent { code: KeyCode::Enter, .. }) => {
                                        selected = true;
                                    },
                                    Event::Key(KeyEvent { code: KeyCode::Esc, .. }) => {
                                        // 取消选择
                                        // 恢复终端模式会通过RawModeGuard的Drop实现自动处理
                                        execute!(io::stdout(),
                                            MoveToColumn(0),
                                            Print("\n已取消模型切换\n".yellow())
                                        )?;
                                        continue 'outer;
                                    },
                                    _ => {}
                                }
                                
                                // 使用 crossterm 的光标控制功能移动到列表开始处
                                execute!(io::stdout(), MoveUp(matches.len() as u16))?;
                                
                                // 重新显示选择
                                for (i, model) in matches.iter().enumerate() {
                                    // 清除整行并重新显示
                                    execute!(io::stdout(), 
                                        MoveToColumn(0),
                                        Clear(ClearType::CurrentLine)
                                    )?;
                                    
                                    if i == selected_index {
                                        execute!(io::stdout(),
                                            MoveToColumn(0),
                                            Print(format!("{} {} ({})", ">".green(), model.name, model.provider)),
                                            Print("\n")
                                        )?;
                                    } else {
                                        execute!(io::stdout(),
                                            MoveToColumn(0),
                                            Print(format!("  {} ({})", model.name, model.provider)),
                                            Print("\n")
                                        )?;
                                    }
                                }
                            }
                        }
                        
                        // 恢复终端模式会通过RawModeGuard的Drop实现自动处理
                        
                        // 设置选中的模型
                        let model = &matches[selected_index];
                        current_model = model.id.clone();
                        execute!(io::stdout(),
                            MoveToColumn(0),
                            SetForegroundColor(Color::Green),
                            Print("已切换到模型: "),
                            Print(model.name.clone()),
                            Print("\n"),
                            ResetColor
                        )?;
                        continue;
                    }
                }
            }
            
            // 如果没有匹配的模型，则视为普通消息
            execute!(io::stdout(),
                MoveToColumn(0),
                Print(format!("{} ", "AI:".green().bold()))
            )?;
            io::stdout().flush()?;
            
            send_chat_request(client, input, &current_model, &mut message_history).await?;
        }
    }
    
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    let client = reqwest::Client::new();
    interactive_mode(&client).await
}
