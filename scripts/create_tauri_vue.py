#!/usr/bin/env python3
import os
import subprocess
import sys
from typing import Optional

def run_command(command: str, cwd: Optional[str] = None) -> bool:
    """运行命令并返回是否成功"""
    try:
        subprocess.run(command, shell=True, check=True, cwd=cwd)
        return True
    except subprocess.CalledProcessError as e:
        print(f"命令执行失败: {e}")
        return False

def check_prerequisites() -> bool:
    """检查必要的依赖是否已安装"""
    prerequisites = {
        "Node.js": "node --version",
        "npm": "npm --version",
        "Rust": "rustc --version",
        "Cargo": "cargo --version"
    }
    
    all_installed = True
    for name, command in prerequisites.items():
        if not run_command(command):
            print(f"错误: {name} 未安装或未正确配置")
            all_installed = False
    
    return all_installed

def create_project():
    """创建Vue + TypeScript + Tauri项目"""
    # 获取项目名称
    project_name = input("请输入项目名称: ").strip()
    if not project_name:
        print("项目名称不能为空")
        return
    
    # 创建Vue项目
    print(f"\n正在创建Vue项目 {project_name}...")
    if not run_command(f"npm create vue@latest {project_name}"):
        return
    
    # 进入项目目录
    os.chdir(project_name)
    
    # 安装依赖
    print("\n正在安装项目依赖...")
    if not run_command("npm install"):
        return
    
    # 添加Tauri
    print("\n正在添加Tauri...")
    if not run_command("npm install @tauri-apps/cli"):
        return
    
    # 初始化Tauri
    print("\n正在初始化Tauri...")
    if not run_command("npx @tauri-apps/cli init"):
        return
    
    print(f"\n项目 {project_name} 创建成功！")
    print("\n后续步骤:")
    print(f"1. cd {project_name}")
    print("2. npm run tauri dev  # 启动开发环境")
    print("3. npm run tauri build  # 构建应用")

def main():
    print("=== Vue + TypeScript + Tauri 项目创建工具 ===")
    
    if not check_prerequisites():
        print("\n请确保已安装所有必要的依赖后再试。")
        return
    
    create_project()

if __name__ == "__main__":
    main() 