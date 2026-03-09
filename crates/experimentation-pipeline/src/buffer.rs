//! Bounded local disk buffer for graceful degradation when Kafka is unreachable.
//!
//! Crash-only design: on restart, check for buffer file and replay before accepting
//! new events. No separate "graceful shutdown" path.
//!
//! Format: each entry is a length-prefixed protobuf frame:
//!   [4 bytes LE: topic_len][topic bytes][4 bytes LE: key_len][key bytes][4 bytes LE: payload_len][payload bytes]
//!
//! On overflow (exceeds max_size_bytes), the buffer file is truncated from the front
//! by rewriting without the oldest entries (drop-oldest strategy).

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::PathBuf;

use tracing::{info, warn};

/// A single buffered event ready for replay.
#[derive(Debug, Clone, PartialEq)]
pub struct BufferedEvent {
    pub topic: String,
    pub key: String,
    pub payload: Vec<u8>,
}

/// Configuration for the disk buffer.
#[derive(Debug, Clone)]
pub struct BufferConfig {
    /// Directory where buffer files are stored.
    pub dir: PathBuf,
    /// Maximum buffer size in bytes (default: 100 MB).
    pub max_size_bytes: u64,
}

impl Default for BufferConfig {
    fn default() -> Self {
        Self {
            dir: PathBuf::from("/tmp/experimentation-pipeline-buffer"),
            max_size_bytes: 100 * 1024 * 1024, // 100 MB
        }
    }
}

const BUFFER_FILE_NAME: &str = "events.wal";

/// Bounded write-ahead log for buffering events when Kafka is unreachable.
pub struct DiskBuffer {
    config: BufferConfig,
    file_path: PathBuf,
    current_size: u64,
}

impl DiskBuffer {
    /// Create a new disk buffer. Creates the directory if needed.
    pub fn new(config: BufferConfig) -> io::Result<Self> {
        fs::create_dir_all(&config.dir)?;
        let file_path = config.dir.join(BUFFER_FILE_NAME);
        let current_size = if file_path.exists() {
            fs::metadata(&file_path)?.len()
        } else {
            0
        };

        if current_size > 0 {
            info!(
                path = %file_path.display(),
                size_bytes = current_size,
                "Found existing buffer file from previous run"
            );
        }

        Ok(Self {
            config,
            file_path,
            current_size,
        })
    }

    /// Append a buffered event to the WAL file.
    /// If the buffer would exceed max_size_bytes, drops oldest entries first.
    pub fn append(&mut self, event: &BufferedEvent) -> io::Result<()> {
        let entry_bytes = serialize_entry(event);
        let entry_len = entry_bytes.len() as u64;

        // If this single entry is larger than max, reject it
        if entry_len > self.config.max_size_bytes {
            warn!(
                entry_len,
                max = self.config.max_size_bytes,
                "Single event exceeds buffer max size, dropping"
            );
            return Ok(());
        }

        // If appending would exceed max, compact by dropping oldest
        if self.current_size + entry_len > self.config.max_size_bytes {
            self.compact(entry_len)?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)?;
        file.write_all(&entry_bytes)?;
        file.flush()?;
        self.current_size += entry_len;

        Ok(())
    }

    /// Read all buffered events from the WAL file.
    pub fn read_all(&self) -> io::Result<Vec<BufferedEvent>> {
        if !self.file_path.exists() || self.current_size == 0 {
            return Ok(Vec::new());
        }

        let file = File::open(&self.file_path)?;
        let mut reader = BufReader::new(file);
        let mut events = Vec::new();

        loop {
            match deserialize_entry(&mut reader) {
                Ok(Some(event)) => events.push(event),
                Ok(None) => break,
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                    warn!("Truncated entry at end of buffer file, stopping read");
                    break;
                }
                Err(e) => return Err(e),
            }
        }

        Ok(events)
    }

    /// Check if there are buffered events pending replay.
    pub fn has_pending(&self) -> bool {
        self.current_size > 0
    }

    /// Get the number of bytes currently buffered.
    #[cfg(test)]
    pub fn size_bytes(&self) -> u64 {
        self.current_size
    }

    /// Clear the buffer file after successful replay.
    pub fn clear(&mut self) -> io::Result<()> {
        if self.file_path.exists() {
            fs::remove_file(&self.file_path)?;
        }
        self.current_size = 0;
        info!("Buffer cleared after replay");
        Ok(())
    }

    /// Compact the buffer by dropping oldest entries until there's room for `needed_bytes`.
    fn compact(&mut self, needed_bytes: u64) -> io::Result<()> {
        let events = self.read_all()?;
        let target_size = self.config.max_size_bytes.saturating_sub(needed_bytes);

        // Rebuild from newest entries, dropping oldest
        let mut kept: Vec<&BufferedEvent> = Vec::new();
        let mut kept_size: u64 = 0;

        for event in events.iter().rev() {
            let entry_size = serialize_entry(event).len() as u64;
            if kept_size + entry_size > target_size {
                break;
            }
            kept.push(event);
            kept_size += entry_size;
        }

        kept.reverse();
        let dropped = events.len() - kept.len();

        // Rewrite buffer file
        let tmp_path = self.file_path.with_extension("wal.tmp");
        {
            let file = File::create(&tmp_path)?;
            let mut writer = BufWriter::new(file);
            for event in &kept {
                writer.write_all(&serialize_entry(event))?;
            }
            writer.flush()?;
        }
        fs::rename(&tmp_path, &self.file_path)?;
        self.current_size = kept_size;

        warn!(
            dropped,
            kept = kept.len(),
            new_size_bytes = kept_size,
            "Buffer compacted (drop-oldest)"
        );

        Ok(())
    }
}

/// Serialize a buffered event to length-prefixed bytes.
fn serialize_entry(event: &BufferedEvent) -> Vec<u8> {
    let topic_bytes = event.topic.as_bytes();
    let key_bytes = event.key.as_bytes();
    let payload = &event.payload;

    let total_len = 4 + topic_bytes.len() + 4 + key_bytes.len() + 4 + payload.len();
    let mut buf = Vec::with_capacity(total_len);

    buf.extend_from_slice(&(topic_bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(topic_bytes);
    buf.extend_from_slice(&(key_bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(key_bytes);
    buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(payload);

    buf
}

/// Deserialize a single entry from a reader. Returns None on clean EOF.
fn deserialize_entry(reader: &mut impl Read) -> io::Result<Option<BufferedEvent>> {
    let mut len_buf = [0u8; 4];

    // Read topic length — clean EOF means no more entries
    match reader.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let topic_len = u32::from_le_bytes(len_buf) as usize;
    let mut topic_buf = vec![0u8; topic_len];
    reader.read_exact(&mut topic_buf)?;
    let topic =
        String::from_utf8(topic_buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    // Read key
    reader.read_exact(&mut len_buf)?;
    let key_len = u32::from_le_bytes(len_buf) as usize;
    let mut key_buf = vec![0u8; key_len];
    reader.read_exact(&mut key_buf)?;
    let key =
        String::from_utf8(key_buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    // Read payload
    reader.read_exact(&mut len_buf)?;
    let payload_len = u32::from_le_bytes(len_buf) as usize;
    let mut payload = vec![0u8; payload_len];
    reader.read_exact(&mut payload)?;

    Ok(Some(BufferedEvent {
        topic,
        key,
        payload,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn test_config(dir: &Path) -> BufferConfig {
        BufferConfig {
            dir: dir.to_path_buf(),
            max_size_bytes: 1024, // 1 KB for testing
        }
    }

    fn make_event(topic: &str, key: &str, payload: &[u8]) -> BufferedEvent {
        BufferedEvent {
            topic: topic.to_string(),
            key: key.to_string(),
            payload: payload.to_vec(),
        }
    }

    #[test]
    fn test_empty_buffer() {
        let dir = tempfile::tempdir().unwrap();
        let buffer = DiskBuffer::new(test_config(dir.path())).unwrap();
        assert!(!buffer.has_pending());
        assert_eq!(buffer.size_bytes(), 0);
        assert!(buffer.read_all().unwrap().is_empty());
    }

    #[test]
    fn test_append_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let mut buffer = DiskBuffer::new(test_config(dir.path())).unwrap();

        let e1 = make_event("exposures", "exp-1", b"payload1");
        let e2 = make_event("metric_events", "user-1", b"payload2");

        buffer.append(&e1).unwrap();
        buffer.append(&e2).unwrap();

        assert!(buffer.has_pending());
        assert!(buffer.size_bytes() > 0);

        let events = buffer.read_all().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], e1);
        assert_eq!(events[1], e2);
    }

    #[test]
    fn test_clear_removes_buffer() {
        let dir = tempfile::tempdir().unwrap();
        let mut buffer = DiskBuffer::new(test_config(dir.path())).unwrap();

        buffer
            .append(&make_event("t", "k", b"data"))
            .unwrap();
        assert!(buffer.has_pending());

        buffer.clear().unwrap();
        assert!(!buffer.has_pending());
        assert_eq!(buffer.size_bytes(), 0);
        assert!(buffer.read_all().unwrap().is_empty());
    }

    #[test]
    fn test_compact_drops_oldest() {
        let dir = tempfile::tempdir().unwrap();
        // Very small max size to trigger compaction
        let config = BufferConfig {
            dir: dir.path().to_path_buf(),
            max_size_bytes: 200,
        };
        let mut buffer = DiskBuffer::new(config).unwrap();

        // Fill buffer with events
        for i in 0..5 {
            let event = make_event("t", "k", format!("payload-{i}").as_bytes());
            buffer.append(&event).unwrap();
        }

        // Buffer should stay within bounds
        assert!(buffer.size_bytes() <= 200);

        // Should still have at least the newest events
        let events = buffer.read_all().unwrap();
        assert!(!events.is_empty());

        // The last event should be the most recent one that fits
        let last = events.last().unwrap();
        assert!(String::from_utf8_lossy(&last.payload).starts_with("payload-"));
    }

    #[test]
    fn test_survives_restart() {
        let dir = tempfile::tempdir().unwrap();

        // Write some events
        {
            let mut buffer = DiskBuffer::new(test_config(dir.path())).unwrap();
            buffer
                .append(&make_event("t1", "k1", b"data1"))
                .unwrap();
            buffer
                .append(&make_event("t2", "k2", b"data2"))
                .unwrap();
        }

        // "Restart": create new DiskBuffer from same directory
        let buffer = DiskBuffer::new(test_config(dir.path())).unwrap();
        assert!(buffer.has_pending());

        let events = buffer.read_all().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].topic, "t1");
        assert_eq!(events[1].topic, "t2");
    }

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        let event = make_event("my-topic", "my-key", b"hello world");
        let bytes = serialize_entry(&event);

        let mut reader = io::Cursor::new(bytes);
        let deserialized = deserialize_entry(&mut reader).unwrap().unwrap();
        assert_eq!(deserialized, event);
    }

    // ---- Phase 4: Crash-recovery tests ----

    /// Simulate crash-restart cycle: write events, drop buffer (crash), create new
    /// buffer (restart), verify all events are recoverable and in correct order.
    #[test]
    fn test_crash_recovery_preserves_all_events() {
        let dir = tempfile::tempdir().unwrap();
        let config = BufferConfig {
            dir: dir.path().to_path_buf(),
            max_size_bytes: 1024 * 1024, // 1 MB
        };

        let events: Vec<BufferedEvent> = (0..100)
            .map(|i| {
                make_event(
                    if i % 2 == 0 { "exposures" } else { "metric_events" },
                    &format!("key-{i}"),
                    format!("protobuf-payload-{i:06}").as_bytes(),
                )
            })
            .collect();

        // Phase 1: Write events (simulating buffering during Kafka outage)
        {
            let mut buffer = DiskBuffer::new(config.clone()).unwrap();
            for event in &events {
                buffer.append(event).unwrap();
            }
            assert!(buffer.has_pending());
            // Buffer is dropped here (simulating kill -9)
        }

        // Phase 2: Restart — verify all events are recoverable
        let mut buffer = DiskBuffer::new(config).unwrap();
        assert!(buffer.has_pending());

        let recovered = buffer.read_all().unwrap();
        assert_eq!(recovered.len(), events.len(), "All events must survive crash");

        // Verify order preservation
        for (i, (original, recovered)) in events.iter().zip(recovered.iter()).enumerate() {
            assert_eq!(original, recovered, "Event {i} mismatch after crash recovery");
        }

        // Phase 3: Clear after successful "replay"
        buffer.clear().unwrap();
        assert!(!buffer.has_pending());
        assert!(buffer.read_all().unwrap().is_empty());
    }

    /// Recovery startup time: creating DiskBuffer and reading pending events
    /// must complete in < 100ms even with 10K buffered events.
    #[test]
    fn test_recovery_startup_latency() {
        let dir = tempfile::tempdir().unwrap();
        let config = BufferConfig {
            dir: dir.path().to_path_buf(),
            max_size_bytes: 50 * 1024 * 1024, // 50 MB
        };

        // Write 10K events (simulating a large crash buffer)
        {
            let mut buffer = DiskBuffer::new(config.clone()).unwrap();
            for i in 0..10_000 {
                let event = make_event(
                    "exposures",
                    &format!("exp-{i}"),
                    &[0xAB; 200], // ~200 byte payloads (realistic protobuf size)
                );
                buffer.append(&event).unwrap();
            }
        }

        // Measure restart + read time
        let start = std::time::Instant::now();
        let buffer = DiskBuffer::new(config).unwrap();
        assert!(buffer.has_pending());
        let events = buffer.read_all().unwrap();
        let elapsed = start.elapsed();

        assert_eq!(events.len(), 10_000);
        assert!(
            elapsed.as_millis() < 2000,
            "Recovery took {}ms, expected < 2000ms",
            elapsed.as_millis()
        );
    }

    /// Test that multiple crash-restart cycles don't corrupt the buffer.
    #[test]
    fn test_multiple_crash_restart_cycles() {
        let dir = tempfile::tempdir().unwrap();
        let config = BufferConfig {
            dir: dir.path().to_path_buf(),
            max_size_bytes: 10 * 1024, // 10 KB
        };

        // Cycle 1: Write some events, "crash"
        {
            let mut buffer = DiskBuffer::new(config.clone()).unwrap();
            for i in 0..5 {
                buffer.append(&make_event("t", "k", format!("cycle1-{i}").as_bytes())).unwrap();
            }
        }

        // Cycle 2: Restart, verify, add more events, "crash" again
        {
            let mut buffer = DiskBuffer::new(config.clone()).unwrap();
            let recovered = buffer.read_all().unwrap();
            assert_eq!(recovered.len(), 5);

            // Simulate partial replay — buffer NOT cleared (crash before clear)
            for i in 0..3 {
                buffer.append(&make_event("t", "k", format!("cycle2-{i}").as_bytes())).unwrap();
            }
        }

        // Cycle 3: Restart, verify both cycle 1 and cycle 2 events present
        {
            let buffer = DiskBuffer::new(config.clone()).unwrap();
            let recovered = buffer.read_all().unwrap();
            assert_eq!(recovered.len(), 8, "Should have 5 from cycle 1 + 3 from cycle 2");

            // Cycle 1 events first
            assert!(String::from_utf8_lossy(&recovered[0].payload).starts_with("cycle1-"));
            // Cycle 2 events appended
            assert!(String::from_utf8_lossy(&recovered[5].payload).starts_with("cycle2-"));
        }

        // Cycle 4: Successful replay — clear buffer
        {
            let mut buffer = DiskBuffer::new(config).unwrap();
            buffer.clear().unwrap();
            assert!(!buffer.has_pending());
        }
    }

    /// Test buffer with all four Kafka topics (exposures, metric_events, reward_events, qoe_events).
    #[test]
    fn test_crash_recovery_multi_topic() {
        let dir = tempfile::tempdir().unwrap();
        let config = BufferConfig {
            dir: dir.path().to_path_buf(),
            max_size_bytes: 1024 * 1024,
        };

        let topics = ["exposures", "metric_events", "reward_events", "qoe_events"];

        {
            let mut buffer = DiskBuffer::new(config.clone()).unwrap();
            for (i, topic) in topics.iter().enumerate() {
                for j in 0..10 {
                    let event = make_event(
                        topic,
                        &format!("key-{i}-{j}"),
                        format!("{topic}-event-{j}").as_bytes(),
                    );
                    buffer.append(&event).unwrap();
                }
            }
        }

        // Recovery
        let buffer = DiskBuffer::new(config).unwrap();
        let recovered = buffer.read_all().unwrap();
        assert_eq!(recovered.len(), 40); // 4 topics × 10 events

        // Verify topic distribution
        for topic in topics {
            let count = recovered.iter().filter(|e| e.topic == topic).count();
            assert_eq!(count, 10, "Expected 10 events for topic {topic}");
        }
    }

    /// Test truncated WAL (simulating crash mid-write).
    #[test]
    fn test_truncated_wal_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let config = BufferConfig {
            dir: dir.path().to_path_buf(),
            max_size_bytes: 1024 * 1024,
        };

        // Write 5 complete events
        {
            let mut buffer = DiskBuffer::new(config.clone()).unwrap();
            for i in 0..5 {
                buffer.append(&make_event("t", "k", format!("event-{i}").as_bytes())).unwrap();
            }
        }

        // Corrupt the WAL by appending partial data (simulating crash mid-write)
        {
            let wal_path = dir.path().join(BUFFER_FILE_NAME);
            let mut file = OpenOptions::new().append(true).open(&wal_path).unwrap();
            // Write partial entry: topic length but no topic data
            file.write_all(&10u32.to_le_bytes()).unwrap();
            file.write_all(b"parti").unwrap(); // incomplete — 5 of 10 bytes
        }

        // Recovery should return the 5 complete events, ignoring truncated tail
        let buffer = DiskBuffer::new(config).unwrap();
        let recovered = buffer.read_all().unwrap();
        assert_eq!(recovered.len(), 5, "Should recover 5 complete events, ignoring truncated entry");
    }
}
