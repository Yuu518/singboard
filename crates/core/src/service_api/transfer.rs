use super::connection::Client;
use super::proto::*;
use super::stream::Events;
use super::{ApiError, Result};
use serde_json::json;
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

const CHUNK_SIZE: usize = 16 * 1024 - 64;

#[derive(Default)]
struct UploadProgress {
    completed: usize,
    completed_bytes: u64,
}

impl UploadProgress {
    fn record(
        &mut self,
        sizes: &[i64],
        progress: &TaildropSendProgress,
    ) -> Result<serde_json::Value> {
        let Some(&size) = sizes.get(self.completed) else {
            return Err(ApiError::new("protocol", "收到额外的文件进度"));
        };
        if usize::try_from(progress.file_index).ok() != Some(self.completed)
            || !(0..=size).contains(&progress.sent_bytes)
            || (progress.file_completed && progress.sent_bytes != size)
        {
            return Err(ApiError::new("protocol", "文件进度与发送批次不符"));
        }
        let total = self
            .completed_bytes
            .checked_add(progress.sent_bytes as u64)
            .ok_or_else(|| ApiError::new("protocol", "文件总大小溢出"))?;
        if progress.file_completed {
            self.completed += 1;
            self.completed_bytes = total;
        }
        Ok(json!({
            "data": progress,
            "completedFiles": self.completed,
            "totalSentBytes": total,
        }))
    }
}

pub(super) async fn upload(
    mut client: Client,
    endpoint_tag: String,
    peer_stable_id: String,
    paths: Vec<PathBuf>,
    events: &Events,
) -> Result<()> {
    if paths.is_empty() || peer_stable_id.is_empty() {
        return Err(ApiError::invalid("请选择文件和接收设备"));
    }
    let mut files = Vec::new();
    let mut descriptions = Vec::new();
    for path in paths {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| ApiError::invalid("无效的文件名"))?;
        let file = File::open(&path).await?;
        let info = file.metadata().await?;
        if !info.is_file() {
            return Err(ApiError::invalid("请选择普通文件"));
        }
        let size = i64::try_from(info.len()).map_err(ApiError::invalid)?;
        descriptions.push(TaildropOutgoingFile {
            name: name.to_owned(),
            size,
        });
        files.push((file, size));
    }
    let sizes: Vec<_> = descriptions.iter().map(|file| file.size).collect();
    let (tx, rx) = mpsc::channel(8);
    use taildrop_send_client_message::Message;
    let send = async move {
        let message = |body| TaildropSendClientMessage {
            message: Some(body),
        };
        let start = TaildropSendStart {
            endpoint_tag,
            peer_stable_id,
            files: descriptions,
        };
        if tx.send(message(Message::Start(start))).await.is_err() {
            return Ok(());
        }
        let mut buffer = vec![0; CHUNK_SIZE];
        for (mut file, size) in files {
            let mut total = 0i64;
            loop {
                let n = file.read(&mut buffer).await?;
                if n == 0 {
                    break;
                }
                total += n as i64;
                if total > size {
                    return Err(ApiError::invalid("传输期间文件大小发生变化"));
                }
                let chunk = TaildropFileChunk {
                    data: buffer[..n].to_vec(),
                };
                if tx.send(message(Message::Chunk(chunk))).await.is_err() {
                    return Ok(());
                }
            }
            if total != size {
                return Err(ApiError::invalid("传输期间文件大小发生变化"));
            }
            if tx
                .send(message(Message::FileDone(TaildropFileDone {})))
                .await
                .is_err()
            {
                return Ok(());
            }
        }
        Ok(())
    };
    let receive = async {
        let mut replies = client
            .send_taildrop_files(ReceiverStream::new(rx))
            .await?
            .into_inner();
        let mut batch = UploadProgress::default();
        while let Some(reply) = replies.message().await? {
            if let Some(taildrop_send_server_message::Message::Progress(progress)) = reply.message {
                events.update(batch.record(&sizes, &progress)?);
            }
        }
        if batch.completed != sizes.len() {
            return Err(ApiError::new("incomplete", "服务端未确认全部文件"));
        }
        Ok(())
    };
    tokio::try_join!(send, receive)?;
    Ok(())
}

pub(super) async fn download(
    mut client: Client,
    endpoint_tag: String,
    name: String,
    destination: PathBuf,
    events: &Events,
) -> Result<()> {
    let dir = destination
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if destination.try_exists()? {
        return Err(ApiError::new("alreadyExists", "目标文件已存在"));
    }
    let mut chunks = client
        .download_taildrop_file(DownloadTaildropFileRequest { endpoint_tag, name })
        .await?
        .into_inner();
    let header = chunks
        .message()
        .await?
        .ok_or_else(|| ApiError::new("protocol", "缺少文件大小"))?;
    if header.size < 0 || !header.data.is_empty() {
        return Err(ApiError::new("protocol", "文件头无效"));
    }
    let tmp = tempfile::NamedTempFile::new_in(dir)?;
    let (file, temp_path) = tmp.into_parts();
    let mut file = File::from_std(file);
    let mut received = 0i64;
    while let Some(chunk) = chunks.message().await? {
        received = received
            .checked_add(chunk.data.len() as i64)
            .ok_or_else(|| ApiError::new("protocol", "文件大小溢出"))?;
        if received > header.size {
            return Err(ApiError::new("protocol", "下载数据超过声明的文件大小"));
        }
        file.write_all(&chunk.data).await?;
        events.update(json!({"data": {"receivedBytes": received, "size": header.size}}));
    }
    if received != header.size {
        return Err(ApiError::new("incomplete", "文件下载不完整"));
    }
    file.flush().await?;
    drop(file);
    temp_path
        .persist_noclobber(destination)
        .map_err(|e| ApiError::from(e.error))?;
    events.update(
        json!({"data": {"receivedBytes": received, "size": header.size, "completed": true}}),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_includes_completed_files_and_preserves_per_file_counts() {
        let sizes = [8, 0, 5];
        let mut batch = UploadProgress::default();
        for (index, bytes, done, count, total) in [
            (0, 3, false, 0, 3),
            (0, 8, true, 1, 8),
            (1, 0, true, 2, 8),
            (2, 1, false, 2, 9),
            (2, 5, true, 3, 13),
        ] {
            let progress = TaildropSendProgress {
                file_index: index,
                sent_bytes: bytes,
                file_completed: done,
            };
            let event = batch.record(&sizes, &progress).unwrap();
            assert_eq!(event["data"]["sentBytes"], bytes);
            assert_eq!(event["completedFiles"], count);
            assert_eq!(event["totalSentBytes"], total);
        }
    }
}
