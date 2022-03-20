use anyhow::anyhow;
use clap::{arg, Command};
use log::{error, warn};
use std::time::Duration;
use telnet::Event;

#[derive(Clone, Debug)]
struct QueryStatus {
    id: i32,
    msg: String,
}

impl QueryStatus {
    pub fn new(id: i32, msg: String) -> Self {
        Self { id, msg }
    }

    pub fn id(&self) -> i32 {
        self.id
    }
    pub fn msg(&self) -> &str {
        &self.msg
    }

    pub fn is_ok(&self) -> bool {
        self.id == 0
    }
}

impl TryFrom<&str> for QueryStatus {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let (_, line) = value
            .split_once("error ")
            .ok_or_else(|| anyhow!("Split error: {}", value))?;
        let (id, msg) = line
            .split_once(' ')
            .ok_or_else(|| anyhow!("Split error: {}", line))?;
        debug_assert!(id.contains('='));
        debug_assert!(msg.contains('='));
        let (_, id) = id.split_once('=').unwrap();
        let (_, msg) = msg.split_once('=').unwrap();
        Ok(Self::new(
            id.parse()
                .map_err(|e| anyhow!("Got parse error: {:?}", e))?,
            msg.to_string(),
        ))
    }
}

struct TelnetConn {
    conn: telnet::Telnet,
}

impl TelnetConn {
    fn decode_result(data: Box<[u8]>) -> anyhow::Result<Option<QueryStatus>> {
        let content =
            String::from_utf8(data.to_vec()).map_err(|e| anyhow!("Got FromUtf8Error: {:?}", e))?;

        debug_assert!(content.contains("error id="));

        for line in content.lines() {
            if line.starts_with("error ") {
                let status = QueryStatus::try_from(line)?;
                if !status.is_ok() {
                    return Err(anyhow!(
                        "Got non ok status: id={} msg={}",
                        status.id(),
                        status.msg()
                    ));
                }

                return Ok(Some(status));
            }
        }
        Ok(None)
    }

    fn connect(server: &str, port: u16) -> anyhow::Result<Self> {
        let conn = telnet::Telnet::connect((server, port), 512)
            .map_err(|e| anyhow!("Got error while connect to {}:{} {:?}", server, port, e))?;
        let mut self_ = Self { conn };

        let content = self_
            .read_data(1)
            .map_err(|e| anyhow!("Got error while read content: {:?}", e))?;

        if content.is_none() {
            warn!("Read none");
        }

        Ok(self_)
    }

    fn read_data(&mut self, timeout: u64) -> anyhow::Result<Option<Box<[u8]>>> {
        match self
            .conn
            .read_timeout(Duration::from_secs(timeout))
            .map_err(|e| anyhow!("Got error while read data: {:?}", e))?
        {
            Event::Data(data) => Ok(Some(data)),
            Event::TimedOut => Ok(None),
            Event::NoData => Ok(None),
            Event::Error(e) => Err(anyhow!("Got error: {:?}", e)),
            _ => Err(anyhow!("Got unknown error")),
        }
    }

    fn write_data(&mut self, payload: &str) -> anyhow::Result<()> {
        self.conn
            .write(payload.as_bytes())
            .map(|size| {
                if size != payload.as_bytes().len() {
                    error!("Error")
                }
            })
            .map_err(|e| anyhow!("Got error while send data: {:?}", e))?;
        Ok(())
    }

    fn write_and_read(&mut self, payload: &str, timeout: u64) -> anyhow::Result<Box<[u8]>> {
        self.write_data(&payload)?;
        Ok(self
            .read_data(timeout)?
            .ok_or_else(|| anyhow!("Return data is None"))?)
    }

    fn login(&mut self, user: &str, password: &str) -> anyhow::Result<QueryStatus> {
        let payload = format!("login {} {}\n\r", user, password);
        let data = self.write_and_read(payload.as_str(), 2)?;
        Ok(Self::decode_result(data)?.ok_or_else(|| anyhow!("Can't find status line."))?)
    }

    fn select_server(&mut self, server_id: i32) -> anyhow::Result<QueryStatus> {
        let payload = format!("use {}\n\r", server_id);
        let data = self.write_and_read(payload.as_str(), 2)?;
        Ok(Self::decode_result(data)?.ok_or_else(|| anyhow!("Can't find status line."))?)
    }
}

fn staff(server: &str, port: u16, user: &str, password: &str, sid: &str) -> anyhow::Result<()> {
    let mut conn = TelnetConn::connect(server, port)?;
    let status = conn.login(user, password)?;
    if !status.is_ok() {
        return Err(anyhow!("Login failed. {:?}", status));
    }
    let status = conn.select_server(
        sid.parse()
            .map_err(|e| anyhow!("Got error while parse sid: {:?}", e))?,
    )?;
    if !status.is_ok() {
        return Err(anyhow!("Select server id failed: {:?}", status));
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let matches = Command::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .args(&[
            arg!(--server [SERVER] "Teamspeak ServerQuery server address"),
            arg!(--port [PORT] "Teamspeak ServerQuery server port"),
            arg!(<USER> "Teamspeak ServerQuery user"),
            arg!(<PASSWORD> "Teamspeak ServerQuery password"),
            arg!(--sid [SID] "Teamspeak ServerQuery server id"),
        ])
        .get_matches();
    env_logger::Builder::from_default_env().init();
    staff(
        matches.value_of("SERVER").unwrap_or("localhost"),
        matches
            .value_of("PORT")
            .unwrap_or("10011")
            .parse()
            .unwrap_or_else(|e| {
                warn!("Got parse error: {:?}", e);
                10011
            }),
        matches.value_of("USER").unwrap(),
        matches.value_of("PASSWORD").unwrap(),
        matches.value_of("SID").unwrap_or("0"),
    )?;
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_connection() {
        let mut conn = TelnetConn::connect(env!("QUERY_HOST"), 10011).unwrap();

        let result = conn.login("serveradmin", env!("QUERY_PASSWORD")).unwrap();

        assert!(result.is_ok());

        let result = conn.select_server(0).unwrap();
        assert!(result.is_ok())
    }
}
