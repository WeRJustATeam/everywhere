@echo off
chcp 65001 > nul
echo 开始运行P2P消息传递测试...

cd /d %~dp0
echo 当前工作目录: %CD%

echo 编译测试程序...
cargo build

echo 运行消息测试...
cargo test --test message_test -- --nocapture

echo 测试完成。