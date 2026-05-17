@echo off
setlocal EnableExtensions EnableDelayedExpansion
chcp 65001 >nul
title CC Desktop Switch
echo ========================================
echo    CC Desktop Switch v1.0.25
echo    正在启动管理后台...
echo ========================================

cd /d "%~dp0"

if not defined CCDS_CONFIG_DIR (
    set "CCDS_CONFIG_DIR=%~dp0.tmp\dev-config"
    echo [信息] 开发启动默认使用独立配置目录:
    echo        !CCDS_CONFIG_DIR!
)

REM 检查 Python
python --version >nul 2>&1
if %errorlevel% neq 0 (
    echo [错误] 未检测到 Python，请先安装 Python 3.9+
    pause
    exit /b 1
)

REM 检查依赖
pip show fastapi >nul 2>&1
if %errorlevel% neq 0 (
    echo [信息] 首次运行，正在安装依赖...
    pip install -r requirements.txt
    if %errorlevel% neq 0 (
        echo [错误] 依赖安装失败
        pause
        exit /b 1
    )
)

python main.py
if %errorlevel% neq 0 (
    echo [错误] 程序异常退出，错误码: %errorlevel%
    pause
)
