@echo off
title CC Desktop Switch
echo ========================================
echo    CC Desktop Switch v1.0.1
echo    正在启动管理后台...
echo ========================================

cd /d "%~dp0"

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
