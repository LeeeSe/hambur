@echo off
rem 编译 debug 包并自动安装到已连接设备
if not defined JAVA_HOME set "JAVA_HOME=C:\Program Files\Eclipse Adoptium\jdk-25.0.4.7-hotspot"
if not defined ANDROID_HOME set "ANDROID_HOME=D:\Android"
call "%~dp0gradlew.bat" :app:installDebug %*
