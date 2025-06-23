use tokio::net::TcpStream;
use tokio_util::codec::Framed;                
use futures::{SinkExt, StreamExt};            
use std::io::{stdin, stdout, Write};        
use anyhow::Result;
use config::{Config, File};
use serde::Deserialize;
use rustchat::common::{Message, ServerMessage, ClientMessage};
use rustchat::common::codec::LengthCodec;
use crossterm::event::{self, Event, KeyCode}; 

#[derive(Debug, Deserialize)]
struct ClientConfig {
    host: String,
    port: u16,
}

// 读入名字
fn name_prompt(msg: &str) -> std::io::Result<String> {
    print!("{}", msg);
    stdout().flush()?; 
    let mut s = String::new();
    stdin().read_line(&mut s)?;
    Ok(s.trim().to_string())
}

// 读入一行消息
fn read_line() -> std::io::Result<String> {
    let mut s = String::new();
    stdin().read_line(&mut s)?;
    Ok(s.trim().to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    
    let name = name_prompt("Enter your name: ")?;

    // 客户端连接到服务器，一样的逻辑
    let settings = Config::builder()
        .set_default("host", "127.0.0.1")?
        .set_default("port", 8080)?
        .add_source(File::with_name("Config").required(false))
        .build()?;

    let cfg: ClientConfig = settings.try_deserialize()?;
    let server_addr = format!("{}:{}", cfg.host, cfg.port);
    println!("Connecting to server at {}", server_addr);

    // 客户端，启动
    let socket = TcpStream::connect(&server_addr).await?;
    println!("✅ Successfully Connected!");

    let mut framed = Framed::new(socket, LengthCodec);

    // 向服务器注册
    let join_msg = Message::Clientmsg(ClientMessage::Register { name: name.clone() });
    framed.send(join_msg).await?;

    // 分离编码与解码：Sink 用于编码，Stream 用于解码
    let (mut sink, mut stream) = framed.split();

   
    let name_for_recv = name.clone();

    // tokio::spawn 一个任务循环打印所有到来的消息，根据消息类型格式化输出
    tokio::spawn(async move {
        while let Some(Ok(Message::Servermsg(msg))) = stream.next().await {
            match msg {
                ServerMessage::BroadcastMessage { from, content } => {
                    println!("[{}] {}", from, content);
                }
                ServerMessage::PrivateMessage { from, to, content } if to == name_for_recv => {
                    println!("[私聊][{} → you] {}", from, content);
                }
                ServerMessage::UserList { content, to } if to == name_for_recv => {
                    println!("[系统] Userlist:\n {:?}", content);
                }
                ServerMessage::History { content, to} if to == name_for_recv => {
                    println!("[系统] Histroy:\n {}", content);
                }
                ServerMessage::Error { content, to } if to == name_for_recv => {
                    println!("[错误] {}", content);
                }
                ServerMessage::System { content } => {
                    println!("[系统] {}", content);
                }
                ServerMessage::Exit => {
                    println!("[系统] The server is shutting down and the client is about to exit");
                    std::process::exit(0);
                }
                _ => {}
            }
        }
    });

    /* 在主线程里循环监听按键，
        按 q 退出，
        /w <user> <msg>（私聊）
        /users 请求当前用户列表
        /history 请求历史聊天记录, 只能看见广播的消息、自己的请求和与自己相关的私聊消息
        默认群发
        通过 sink.send 发送给服务器
    */
    loop {
        // 每 500ms 检测一次键盘事件
        if event::poll(std::time::Duration::from_millis(500))? {
            if let Event::Key(key_event) = event::read()? {
                
                if key_event.code == KeyCode::Char('q') {
                    break;
                }
                
                let input = read_line()?;
                
                let msg = if input.starts_with("/w ") {
                    let parts: Vec<&str> = input[3..].splitn(2, ' ').collect();
                    Message::Clientmsg(ClientMessage::Private {
                        from: name.clone(),
                        to: parts[0].to_string(),
                        content: parts[1].to_string(),
                    })
                } else if input == "/users"{
                    Message::Clientmsg(ClientMessage::Command { from: name.clone(), command: "/users".to_string()})
                } else if input == "/history"{
                    Message::Clientmsg(ClientMessage::Command { from: name.clone(), command: "/history".to_string()})
                } else{
                    Message::Clientmsg(ClientMessage::Broadcast { from: name.clone(), content: input })
                };
                // 发送消息
                if sink.send(msg).await.is_err() {
                    break;
                }
            }
        }
    }
    println!("{} exit", name);
    Ok(())
}