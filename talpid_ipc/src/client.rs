use serde;
use serde_json;
use std::sync::mpsc;
use url;
use ws;

mod errors {
    error_chain!{}
}
pub use self::errors::*;


struct Factory {
    request: String,
    result_tx: mpsc::Sender<Result<serde_json::Value>>,
}

impl ws::Factory for Factory {
    type Handler = Handler;

    fn connection_made(&mut self, sender: ws::Sender) -> Self::Handler {
        debug!("Sending: {}", self.request);
        if let Err(e) =
            sender.send(&self.request[..]).chain_err(|| "Unable to send jsonrpc request") {
            self.result_tx.send(Err(e)).unwrap();
        }
        Handler {
            sender,
            result_tx: self.result_tx.clone(),
        }
    }
}


struct Handler {
    sender: ws::Sender,
    result_tx: mpsc::Sender<Result<serde_json::Value>>,
}

impl Handler {
    fn validate_reply(&self, msg: ws::Message) -> ws::Result<serde_json::Value> {
        let json: serde_json::Value = match msg {
                ws::Message::Text(s) => serde_json::from_str(&s),
                ws::Message::Binary(b) => serde_json::from_slice(&b),
            }
            .map_err(|e| ws::Error::from(Box::new(e)))?;
        debug!("JSON response: {}", json);
        let result =
            match json {
                    serde_json::Value::Object(mut map) => map.remove("result"),
                    _ => None,
                }
                .ok_or(ws::Error::new(ws::ErrorKind::Protocol, "Invalid reply, no 'result'"),)?;
        // TODO(linus): Properly validate reply
        Ok(result)
    }
}

impl ws::Handler for Handler {
    fn on_message(&mut self, msg: ws::Message) -> ws::Result<()> {
        debug!("WsIpcClient incoming message: {:?}", msg);
        let reply = self.validate_reply(msg)?;
        let close_result = self.sender.close(ws::CloseCode::Normal);
        if let Err(e) = close_result.chain_err(|| "Unable to close WebSocket") {
            self.result_tx.send(Err(e)).unwrap();
        }
        self.result_tx.send(Ok(reply)).unwrap();
        Ok(())
    }
}


pub struct WsIpcClient {
    url: url::Url,
    next_id: i64,
}

impl WsIpcClient {
    pub fn new(server_id: ::IpcServerId) -> Result<Self> {
        let url = url::Url::parse(&server_id).chain_err(|| "Unable to parse server_id as url")?;
        Ok(WsIpcClient { url, next_id: 1 })
    }

    pub fn call<T>(&mut self, method: &str, params: &T) -> Result<serde_json::Value>
        where T: serde::Serialize
    {
        let (result_tx, result_rx) = mpsc::channel();
        let factory = Factory {
            request: self.get_json(method, params),
            result_tx: result_tx,
        };
        let mut ws = ws::WebSocket::new(factory).chain_err(|| "Unable to create WebSocket")?;
        ws.connect(self.url.clone()).chain_err(|| "Unable to connect WebSocket to url")?;
        ws.run().chain_err(|| "Error while running WebSocket event loop")?;

        match result_rx.try_recv() {
            Ok(result) => result,
            Err(_) => bail!("Internal error, no WebSocket status"),
        }
    }

    fn get_json<T>(&mut self, method: &str, params: &T) -> String
        where T: serde::Serialize
    {
        let request_json = json!({
            "jsonrpc": "2.0",
            "id": self.get_id(),
            "method": method,
            "params": params,
        });
        format!("{}", request_json)
    }

    fn get_id(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}


#[cfg(test)]
mod tests {
    extern crate env_logger;
    use super::*;

    // TODO(linus): This is not a test. Just an ugly way to quickly test the client implementation
    #[test]
    #[ignore]
    fn ws_ipc_client_tester() {
        env_logger::init().unwrap();

        let mut ws = WsIpcClient::new("ws://127.0.0.1:INSERT_PORT".to_owned()).unwrap();
        let event = serde_json::Value::String("Up".to_owned());
        let env = serde_json::Value::Object(serde_json::Map::new());
        let params = serde_json::Value::Array(vec![event, env]);
        println!("CALL RESULT: {:?}", ws.call("openvpn_event", &params));
    }
}
