# Hambur - 命令行ChatGPT客户端

一个简单而强大的命令行ChatGPT客户端，支持异步和流式传输，让您在终端中与ChatGPT进行交互。

## 特性

- 支持异步处理，响应迅速
- 流式传输，实时显示ChatGPT的回复
- 交互模式，支持连续对话
- 彩色输出，提升用户体验

## 安装

### 从源码编译

```bash
# 克隆仓库
git clone https://github.com/yourusername/hambur.git
cd hambur

# 编译
cargo build --release

# 安装到系统路径（可选）
cargo install --path .
```

## 配置

1. 复制示例环境配置文件

```bash
cp .env.example .env
```

2. 编辑.env文件，填入您的OpenAI API密钥

```
OPENAI_API_KEY=your_openai_api_key_here
OPENAI_API_BASE=https://api.openai.com/v1  # 可选，默认为OpenAI官方API
OPENAI_MODEL=gpt-3.5-turbo  # 可选，默认为gpt-3.5-turbo
```

## 使用方法

```bash
./hambur
# 或者如果已安装到系统路径
hambur
```

在交互模式下，您可以连续与ChatGPT对话，输入`exit`退出。

## 许可证

MIT