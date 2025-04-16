#!/usr/bin/env python3
import os
import subprocess
import sys
import platform
import shutil
import tempfile
import zipfile
import tarfile
import urllib.request
import json
import re
from typing import Optional, Dict, List, Tuple

def run_command(command: str, cwd: Optional[str] = None) -> bool:
    """运行命令并返回是否成功"""
    try:
        subprocess.run(command, shell=True, check=True, cwd=cwd)
        return True
    except subprocess.CalledProcessError as e:
        print(f"命令执行失败: {e}")
        return False

def check_prerequisites() -> Dict[str, bool]:
    """检查必要的依赖是否已安装"""
    prerequisites = {
        "Node.js": "node --version",
        "pnpm": "pnpm --version",
        "Rust": "rustc --version",
        "Cargo": "cargo --version"
    }
    
    results = {}
    for name, command in prerequisites.items():
        if run_command(command):
            print(f"✓ {name} 已安装")
            results[name] = True
        else:
            print(f"✗ {name} 未安装或未正确配置")
            results[name] = False
    
    return results

def download_file(url: str, output_path: str) -> bool:
    """下载文件到指定路径"""
    try:
        print(f"正在下载: {url}")
        urllib.request.urlretrieve(url, output_path)
        return True
    except Exception as e:
        print(f"下载失败: {e}")
        return False

def install_nodejs() -> bool:
    """安装 Node.js"""
    system = platform.system().lower()
    arch = platform.machine().lower()
    
    if system == "windows":
        # 获取最新的 LTS 版本
        try:
            with urllib.request.urlopen("https://nodejs.org/dist/index.json") as response:
                data = json.loads(response.read().decode())
                lts_versions = [v for v in data if v.get("lts")]
                if not lts_versions:
                    print("无法获取 Node.js LTS 版本信息")
                    return False
                
                latest_lts = lts_versions[0]
                version = latest_lts["version"]
                filename = f"node-{version}-x64.msi"
                url = f"https://nodejs.org/dist/{version}/{filename}"
                
                # 下载并安装
                with tempfile.NamedTemporaryFile(suffix=".msi", delete=False) as temp_file:
                    if not download_file(url, temp_file.name):
                        return False
                    
                    print(f"正在安装 Node.js {version}...")
                    return run_command(f"msiexec /i {temp_file.name} /quiet /norestart")
        except Exception as e:
            print(f"安装 Node.js 失败: {e}")
            return False
    
    elif system == "linux":
        # 使用包管理器安装
        if shutil.which("apt"):
            return run_command("sudo apt update && sudo apt install -y nodejs npm")
        elif shutil.which("yum"):
            return run_command("sudo yum install -y nodejs npm")
        elif shutil.which("dnf"):
            return run_command("sudo dnf install -y nodejs npm")
        elif shutil.which("pacman"):
            return run_command("sudo pacman -S --noconfirm nodejs npm")
        else:
            print("未找到支持的包管理器")
            return False
    
    elif system == "darwin":
        # 使用 Homebrew 安装
        if shutil.which("brew"):
            return run_command("brew install node")
        else:
            print("未找到 Homebrew，请手动安装 Node.js")
            return False
    
    else:
        print(f"不支持的操作系统: {system}")
        return False

def install_pnpm() -> bool:
    """安装 pnpm"""
    if run_command("npm install -g pnpm"):
        print("✓ pnpm 安装成功")
        return True
    else:
        print("✗ pnpm 安装失败")
        return False

def install_rust() -> bool:
    """安装 Rust"""
    system = platform.system().lower()
    
    if system == "windows":
        # 下载并运行 rustup-init.exe
        url = "https://win.rustup.rs/x86_64"
        with tempfile.NamedTemporaryFile(suffix=".exe", delete=False) as temp_file:
            if not download_file(url, temp_file.name):
                return False
            
            print("正在安装 Rust...")
            return run_command(f"{temp_file.name} -y")
    
    elif system == "linux" or system == "darwin":
        # 使用 rustup 安装
        return run_command("curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y")
    
    else:
        print(f"不支持的操作系统: {system}")
        return False

def create_workspace_if_not_exists() -> bool:
    """如果工作空间不存在，则创建"""
    if not os.path.exists("Cargo.toml"):
        print("未找到 Cargo.toml，正在创建工作空间...")
        
        # 创建基本的 Cargo.toml
        with open("Cargo.toml", "w") as f:
            f.write("""[workspace]
resolver = "2"
members = []

# 工作空间依赖，所有成员crate共享
[workspace.dependencies]
tokio = { version = "1.28", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
log = "0.4"
anyhow = "1.0"
thiserror = "1.0"
async-trait = "0.1"
uuid = { version = "1.3", features = ["v4", "serde"] }

# 工作空间全局设置
[workspace.package]
authors = ["Your Name <your.email@example.com>"]
edition = "2021"
rust-version = "1.70"
""")
        return True
    
    # 检查是否已经是工作空间
    with open("Cargo.toml", "r") as f:
        content = f.read()
        if "[workspace]" not in content:
            print("Cargo.toml 不是工作空间，正在转换为工作空间...")
            
            # 备份原始文件
            shutil.copy("Cargo.toml", "Cargo.toml.bak")
            
            # 读取原始包信息
            package_name = None
            package_version = None
            package_authors = None
            package_edition = None
            
            # 提取包信息
            name_match = re.search(r'name\s*=\s*"([^"]+)"', content)
            if name_match:
                package_name = name_match.group(1)
            
            version_match = re.search(r'version\s*=\s*"([^"]+)"', content)
            if version_match:
                package_version = version_match.group(1)
            
            authors_match = re.search(r'authors\s*=\s*\[(.*?)\]', content, re.DOTALL)
            if authors_match:
                package_authors = authors_match.group(1)
            
            edition_match = re.search(r'edition\s*=\s*"([^"]+)"', content)
            if edition_match:
                package_edition = edition_match.group(1)
            
            # 创建工作空间配置
            with open("Cargo.toml", "w") as f:
                f.write("""[workspace]
resolver = "2"
members = []

# 工作空间依赖，所有成员crate共享
[workspace.dependencies]
tokio = { version = "1.28", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
log = "0.4"
anyhow = "1.0"
thiserror = "1.0"
async-trait = "0.1"
uuid = { version = "1.3", features = ["v4", "serde"] }

# 工作空间全局设置
[workspace.package]
authors = ["Your Name <your.email@example.com>"]
edition = "2021"
rust-version = "1.70"
""")
            
            # 创建原始包的目录
            if package_name:
                os.makedirs(package_name, exist_ok=True)
                
                # 将原始 Cargo.toml 移动到包目录
                with open(f"{package_name}/Cargo.toml", "w") as f:
                    f.write(f"""[package]
name = "{package_name}"
version = "{package_version or '0.1.0'}"
authors = [{package_authors or '"Your Name <your.email@example.com>"'}]
edition = "{package_edition or '2021'}"
""")
                
                # 更新工作空间成员
                with open("Cargo.toml", "r") as f:
                    content = f.read()
                
                with open("Cargo.toml", "w") as f:
                    f.write(content.replace('members = []', f'members = ["{package_name}"]'))
            
            return True
    
    return True

def add_to_workspace(project_path: str) -> bool:
    """将项目添加到工作空间"""
    if not os.path.exists("Cargo.toml"):
        if not create_workspace_if_not_exists():
            return False
    
    # 读取 Cargo.toml
    with open("Cargo.toml", "r") as f:
        content = f.read()
    
    # 检查项目是否已经是成员
    if f'members = ["{project_path}"]' in content or f'members = ["{project_path}",' in content or f', "{project_path}"]' in content or f', "{project_path}",' in content:
        print(f"项目 {project_path} 已经是工作空间成员")
        return True
    
    # 添加项目到成员列表
    if 'members = []' in content:
        new_content = content.replace('members = []', f'members = ["{project_path}"]')
    elif 'members = [' in content:
        # 在最后一个成员后添加逗号和换行
        new_content = content.replace(']', f',\n    "{project_path}"]')
    else:
        print("无法解析 Cargo.toml 中的成员列表")
        return False
    
    # 写入更新后的 Cargo.toml
    with open("Cargo.toml", "w") as f:
        f.write(new_content)
    
    print(f"已将 {project_path} 添加到工作空间")
    return True

def create_project():
    """创建Vue + TypeScript + Tauri项目"""
    # 获取项目名称
    project_name = input("请输入项目名称: ").strip()
    if not project_name:
        print("项目名称不能为空")
        return
    
    # 检查是否作为工作空间子项目
    as_workspace_member = input("是否将此项目作为工作空间子项目? (y/n): ").strip().lower() == 'y'
    
    # 创建Vue项目
    print("\n正在创建Vue项目...")
    if not run_command(f"pnpm create vue@latest {project_name}"):
        return
    
    # 进入项目目录
    os.chdir(project_name)
    
    # 安装依赖
    print("\n正在安装项目依赖...")
    if not run_command("pnpm install"):
        return
    
    # 添加Tauri
    print("\n正在添加Tauri...")
    if not run_command("pnpm add -D @tauri-apps/cli"):
        return
    
    # 初始化Tauri
    print("\n正在初始化Tauri...")
    if not run_command("pnpm tauri init"):
        return
    
    # 如果作为工作空间子项目，添加到工作空间
    if as_workspace_member:
        # 返回到工作空间根目录
        os.chdir("..")
        
        # 添加到工作空间
        tauri_path = f"{project_name}/src-tauri"
        if not add_to_workspace(tauri_path):
            print(f"无法将 {tauri_path} 添加到工作空间")
            return
        
        # 返回到项目目录
        os.chdir(project_name)
    
    print(f"\n项目 {project_name} 创建成功！")
    print("\n后续步骤:")
    print(f"1. cd {project_name}")
    print("2. pnpm tauri dev  # 启动开发环境")
    print("3. pnpm tauri build  # 构建应用")
    
    if as_workspace_member:
        print("\n注意: 此项目已作为工作空间子项目添加。")
        print("如果遇到依赖问题，请确保工作空间中的所有依赖项都可用。")

def main():
    print("=== Vue + TypeScript + Tauri 项目创建工具 ===")
    
    # 检查依赖
    dependencies = check_prerequisites()
    
    # 安装缺失的依赖
    if not dependencies["Node.js"]:
        print("\n正在安装 Node.js...")
        if not install_nodejs():
            print("Node.js 安装失败，请手动安装")
            return
    
    if not dependencies["pnpm"]:
        print("\n正在安装 pnpm...")
        if not install_pnpm():
            print("pnpm 安装失败，请手动安装")
            return
    
    if not dependencies["Rust"] or not dependencies["Cargo"]:
        print("\n正在安装 Rust...")
        if not install_rust():
            print("Rust 安装失败，请手动安装")
            return
    
    # 创建项目
    create_project()

if __name__ == "__main__":
    main() 