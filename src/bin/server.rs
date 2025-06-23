use tokio::{net::{TcpListener, TcpStream}, sync::Mutex};
use tokio_util::codec::Framed;                
use futures::{SinkExt, StreamExt};          
use anyhow::Result;                           
use std::{sync::Arc, collections::HashMap};
use std::collections::VecDeque;
use tokio::sync::mpsc;
use config::{Config, File};
use serde::Deserialize;                        
use rustchat::common::{Message, ServerMessage, ClientMessage};
use rustchat::common::codec::LengthCodec;

const MAX_HISTORY_SIZE: usize = 100;

/* 共享服务器状态
    clients: 所有已连接的客户端维护“用户名 -> 发送通道”的映射，用于确定消息的接收方
    broadcast_history: 所有广播的消息
    private_history: 私聊消息, 且按客户分开存放
*/
struct ServerState {
    clients: HashMap<String, mpsc::Sender<Message>>,
    broadcast_history: VecDeque<String>, 
    private_history: HashMap<String, VecDeque<String>>,
}
impl Default for ServerState {
    fn default() -> Self { ServerState { 
        clients: HashMap::new(),
        broadcast_history: VecDeque::with_capacity(MAX_HISTORY_SIZE),
        private_history: HashMap::new()
    } }
}

// 服务器的监听地址和段靠谱
#[derive(Debug, Deserialize)]
struct ServerConfig {
    host: String,
    port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    // 服务器绑定 TCP 接口
    let settings = Config::builder()
        // 默认IP和端口
        .set_default("host", "0.0.0.0")?
        .set_default("port", 8080)?
        //再看当前目录下是否有 Config.toml（可选）去合并
        .add_source(File::with_name("Config").required(false))
        .build()?;

    let cfg: ServerConfig = settings.try_deserialize()?;
    let bind_addr = format!("{}:{}", cfg.host, cfg.port);

    // 服务器，启动
    let listener = TcpListener::bind(&bind_addr).await?;
    println!("Server is up on {}", bind_addr);

    let state = Arc::new(Mutex::new(ServerState::default()));

    // 服务器关闭信号：Ctrl+C
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);

    /* 接受新信号：
        如果是新连接，则用 tokio::spawn 为每个客户端开一个任务
        如果是Ctrl+C，则关闭服务器
    */
    loop {
        tokio::select! {
            accept_res = listener.accept() => {
                match accept_res {
                    Ok((socket, addr)) => {
                        println!("New connection: {}", addr);
                        let state = state.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_client(socket, state).await {
                                eprintln!("Client handle error: {}", e);
                            }
                        });
                    }
                    Err(e) => eprintln!("Accept error: {}", e),
                }
            }
            _ = &mut shutdown => {
                println!("Ctrl+C received, shutting down server...");

                let clients = state.lock().await.clients.clone();
                for (_name, tx) in clients {
                    let shutdown_msg = Message::Servermsg(ServerMessage::Exit);
                    let _ = tx.send(shutdown_msg).await;
                }
                
                // 清空 clients，使写任务自然终止
                state.lock().await.clients.clear();
                
                // 等待一小段时间，确保通知下发完毕
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                break;
            }
        }
    }  
    Ok(())
}

// 处理单个客户端连接
async fn handle_client(socket: TcpStream, state: Arc<Mutex<ServerState>>) -> Result<()> {
    // 使用在common.rs中定义的编解码器
    let mut framed = Framed::new(socket, LengthCodec);

    if let Some(Ok(msg)) = framed.next().await {
        // 独立处理第一则消息，因此第一次通信是 Reegister 消息，用存储册用户名和发送通道
        if let Message::Clientmsg(ClientMessage::Register { name }) = msg {
            // 注册用户，并在服务器中储存发送端tx
            let (tx, mut rx) = mpsc::channel(100);
            state.lock().await.clients.insert(name.clone(), tx);
            // 广播“某用户”加入聊天的消息
            register(&name, &state).await;
            // 分离编码与解码：Sink 用于编码，Stream 用于解码
            let (mut sink, mut stream) = framed.split();
            // rx.recv() 接收该客户端消息并发送给特定的客户端
            tokio::spawn(async move {
                while let Some(msg) = rx.recv().await {
                    if sink.send(msg).await.is_err() {
                        break; 
                    }
                }
            });

            // 读取循环：接收该客户端发来的消息并处理
            while let Some(Ok(Message::Clientmsg(msg))) = stream.next().await {
                match &msg {
                    ClientMessage::Broadcast { .. } => broadcast(msg, &state).await,
                    ClientMessage::Private { .. }   => dispatch(msg, &state).await,
                    ClientMessage::Command { .. }   => command(msg, &state).await,
                    _ => (),
                }
            }

            // 客户端断开，移除状态并广播离开通知(系统消息)
            state.lock().await.clients.remove(&name);
            let leave_msg = Message::Servermsg(ServerMessage::System { content: name.clone() + " leave the chat" });
            for (_name, tx) in state.lock().await.clients.clone() {
                let _ = tx.send(leave_msg.clone()).await;
            }
        }
    }
    Ok(())
}

// 广播消息给所有在线客户端
async fn broadcast(msg: ClientMessage, state: &Arc<Mutex<ServerState>>) {
    if let ClientMessage::Broadcast { from , content } = &msg{
        // 记录客户发言
        {
            let mut st = state.lock().await;
            st.broadcast_history.push_back(format!("{} broadcast: {}", from, content));
            if st.broadcast_history.len() > MAX_HISTORY_SIZE {
                st.broadcast_history.pop_front();
            }
        }
        
        // 将广播消息放入mpsc::channel中
        let reply_msg = Message::Servermsg(ServerMessage::BroadcastMessage { from: from.clone(), content: content.clone() });
        let clients = state.lock().await.clients.clone();
        for (_name, tx) in clients {
            let _ = tx.send(reply_msg.clone()).await;
        }
    }
}

// 私聊仅发送给指定目标用户
async fn dispatch(msg: ClientMessage, state: &Arc<Mutex<ServerState>>) {
    if let ClientMessage::Private { from, to, content} = &msg {
        // 记录客户发言(自己发送的 + 送向自己的)
        {
            let mut st = state.lock().await;
            let entry_from = st.private_history
                .entry(from.clone())
                .or_default();
            entry_from.push_back(format!("You → {}: {}", to, content));
            if entry_from.len() > MAX_HISTORY_SIZE {
                entry_from.pop_front();
            }
            let entry_to = st.private_history
                .entry(to.clone())
                .or_default();
            entry_to.push_back(format!("{} → You: {}", from, content));
            if entry_to.len() > MAX_HISTORY_SIZE {
                entry_to.pop_front();
            }
        }

        // 将私聊消息放入mpsc::channel中
        let reply_msg = Message::Servermsg(ServerMessage::PrivateMessage { from: from.clone(), to: to.clone(), content: content.clone() });
        if let Some(tx) = state.lock().await.clients.get(to) {
            let _ = tx.send(reply_msg.clone()).await;
        }else{  // 如果找不到私聊对象, 向该客户端返回一个错误消息
            if let Some(tx) = state.lock().await.clients.get(from){
                let private_error_msg = Message::Servermsg(ServerMessage::Error { content: "Private object is not online or the name is incorrect ".to_string(), to: from.to_string()});
                let _ = tx.send(private_error_msg).await;
            }
        }
    }
}

// 命令
async fn command(msg: ClientMessage, state: &Arc<Mutex<ServerState>>) {
    if let ClientMessage::Command { from, command } = &msg {
        if command == "/users" {
            // 记录客户这次请求
            {
                let mut st = state.lock().await;
                let entry_from = st.private_history
                    .entry(from.clone())
                    .or_default();
                entry_from.push_back(format!("You issued: {}", command));
                if entry_from.len() > MAX_HISTORY_SIZE {
                    entry_from.pop_front();
                }
            }
            
            // 从 clients 整理得到用户列表 user_list, 放入 mpsc::channel 中
            let clients = state.lock().await.clients.clone();
            let mut user_list: Vec<String> = Vec::new();

            for (name, _tx) in clients {
                user_list.push(name.clone());
            }

            let reply_msg = if user_list.is_empty() {
                Message::Servermsg(ServerMessage::System { content: "No User Online".to_string() })
            } else {
                Message::Servermsg(ServerMessage::UserList { content: user_list, to: from.to_string()})
            };

            if let Some(tx) = state.lock().await.clients.get(from) {
                let _ = tx.send(reply_msg).await;
            }
        }else if command == "/history" {
            let mut st = state.lock().await;
            // 记录客户这次请求
            st.private_history
                .entry(from.clone())
                .or_default()
                .push_back(format!("You issued: {}", command));
            // 收集历史: 广播 + 自己的私聊
            let mut lines = Vec::new();
            lines.push("=== Broadcast History ===".into());
            lines.extend(st.broadcast_history.iter().cloned());
            lines.push("=== Your Private History ===".into());
            if let Some(priv_h) = st.private_history.get(from) {
                lines.extend(priv_h.iter().cloned());
            }
            let history_txt = lines.join("\n");

            if let Some(tx) = st.clients.get(from) {
                let _ = tx.send(Message::Servermsg(ServerMessage::History {
                    content: history_txt,
                    to: from.to_string(),
                })).await;
            }
        }else{
            let userlist_error_msg = Message::Servermsg(ServerMessage::Error { content: "No User Online".to_string(), to: from.to_string()});
            if let Some(tx) = state.lock().await.clients.get(from) {
                let _ = tx.send(userlist_error_msg).await;
            }
        }
    }
}

// 注册, 以系统消息形式通知某位客户端上线
async fn register(name: &String, state: &Arc<Mutex<ServerState>>) {
    let clients = state.lock().await.clients.clone();
    let reply_msg = Message::Servermsg(ServerMessage::System {content : name.to_string() + " join the chat"});
    for (_name, tx) in clients {
        let _ = tx.send(reply_msg.clone()).await;
    }
}