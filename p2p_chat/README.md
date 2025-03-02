# P2P 聊天程序

这是一个简单的P2P聊天程序，支持多个终端进程之间相互交换消息。

## 功能

- 基于WebSocket的P2P通信
- 消息广播到所有已连接的节点
- 使用单一YAML配置文件定义所有节点信息
- 去重处理，避免消息循环传播

## 使用方法

1. 编译程序：

```
cargo build --release
```

2. 准备配置文件 (config.yaml)：

```yaml
# P2P聊天程序配置
nodes:
  node1:  # 节点ID
    bind_address: "127.0.0.1:8080"  # 绑定地址
  node2:  # 节点ID
    bind_address: "127.0.0.1:8081"  # 绑定地址
  node3:  # 节点ID
    bind_address: "127.0.0.1:8082"  # 绑定地址
```

3. 在不同的终端窗口中启动节点，指定节点ID：

```
# 终端1
cargo run -- config.yaml node1

# 终端2
cargo run -- config.yaml node2

# 终端3
cargo run -- config.yaml node3
```

4. 或者使用批处理文件一键启动多个节点：

```
run_nodes.bat
```

5. 在任一终端中输入消息，消息将在所有连接的节点上显示。

6. 输入 `exit` 可以退出程序。

## 配置文件说明

YAML配置文件使用以下格式：

```yaml
nodes:
  node_id1:  # 节点ID
    bind_address: "IP地址:端口"  # 绑定地址
  node_id2:
    bind_address: "IP地址:端口"
  # ... 更多节点
```

## 使用参数

程序接受两个命令行参数：
- 配置文件路径
- 节点ID（必须在配置文件中定义）

## 注意事项

- 确保每个节点使用唯一的监听地址
- 确保所有节点的node_id唯一
- 程序启动时会自动尝试连接配置中的其他节点 