use serde::{Serialize, Deserialize};

// 客户端发给服务器的消息类型枚举
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ClientMessage {
    Broadcast {             // 群发
        from: String,
        content: String,
    },
    Private {               // 私聊
        from: String,
        to: String,
        content: String,
    },
    Command {               // 指令, "/users", "/history"
        from: String,
        command: String, 
    },
    Register {              // 注册
        name: String,
    },
}
// 服务器发给客户端的消息类型枚举
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ServerMessage {
    BroadcastMessage {      // 群发
        from: String,
        content: String,
    },
    PrivateMessage {        // 私聊
        from: String,
        to: String,
        content: String,
    },
    UserList {              // 告知用户列表
        content: Vec<String>,
        to: String
    },
    Error {                 // 错误
        content: String,
        to: String
    }, 
    System {                // 系统消息
        content: String,
    },
    History {               // 告知历史记录
        content: String,
        to: String,
    },
    Exit,                   // 服务器关闭
}
// 聊天消息结构体
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Message {
    Clientmsg(ClientMessage),
    Servermsg(ServerMessage),
}

// Codec 模块：基于长度前缀的编码器和解码器
pub mod codec {
    use super::Message;
    use bytes::{BytesMut, Buf, BufMut};
    use serde_json;
    use tokio_util::codec::{Decoder, Encoder};

    // 自定义长度前缀编码器
    pub struct LengthCodec;

    impl Decoder for LengthCodec {
        type Item = Message;
        type Error = std::io::Error;

        // 解码：尝试从 buf 中读取一帧完整消息，将字节流 BytesMut 转化为储存消息内容的JSON对象
        fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Message>, std::io::Error> {
            //每一帧消息长度必须大于等于4且实际长度与长度前缀相匹配(保证取出来的是正确且完整的消息)
            if src.len() < 4 { return Ok(None); }             
            let len = u32::from_be_bytes([src[0], src[1], src[2], src[3]]) as usize;          
            if src.len() < 4 + len { return Ok(None); }
           
            src.advance(4);
            let data = src.split_to(len);          
            let msg: Message = serde_json::from_slice(&data)?;
            Ok(Some(msg))
        }
    }

    impl Encoder<Message> for LengthCodec {
        type Error = std::io::Error;

        // 编码：将 message 序列化并前置长度，储存于 BytesMut 中
        fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<(), std::io::Error> {
            let data = serde_json::to_vec(&item)?;   
            dst.put_u32(data.len() as u32);        
            dst.extend_from_slice(&data);    
            Ok(())
        }
    }
}