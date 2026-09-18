# Hambur Project Guidelines & Agent Memory

## 1. 设备部署与安装规范 (Deployment Memory)

- **小更新（日常改动、小修复、特性调试）**：
  - 任务完成后，通过 ADB 编译并安装 **Debug** 包到已连接的手机：
  - Windows 命令：`cmd /c "gradlew.bat :app:installDebug"` 或执行 `build-install.bat`
  - Linux/macOS 命令：`./build-install.sh :app:installDebug`
- **大更新（核心重构、大版本迭代、重大功能上线）**：
  - 任务完成后，通过 ADB 编译并安装 **Release** 包到已连接的手机：
  - Windows 命令：`cmd /c "gradlew.bat :app:installRelease"`
  - Linux/macOS 命令：`./build-install.sh :app:installRelease`

## 2. 坐标与定位约定 (Location Memory)
- 底层 `android_cli (get_location)` 已内置 WGS-84 到 GCJ-02 转换算法。
- 国内地图服务（高德、腾讯等）直接读取返回结果中的 `gcj02` 对象，无需在上层或技能 Prompt 中做二次转换。
