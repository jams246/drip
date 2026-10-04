use crate::native_abi::quote_utf16;
use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write;
use std::sync::{Mutex, OnceLock};

pub struct Record {
    pub id: usize,
    pub sequence: u64,
    pub action: u32,
    pub path: Vec<u16>,
    pub error: u32,
    pub loss: bool,
}

#[derive(Default)]
pub struct Queue {
    details: VecDeque<Record>,
    bytes: usize,
    losses: BTreeMap<usize, (u64, u32)>,
}

impl Queue {
    pub fn push(&mut self, record: Record) {
        if record.loss {
            self.losses
                .insert(record.id, (record.sequence, record.error));
            return;
        }
        let bytes =
            std::mem::size_of::<Record>() + record.path.capacity() * std::mem::size_of::<u16>();
        if self.details.len() >= 4096 || self.bytes + bytes > 4 * 1024 * 1024 {
            self.losses.insert(record.id, (record.sequence, 0));
            return;
        }
        self.bytes += bytes;
        self.details.push_back(record);
    }

    pub fn drain(&mut self) -> String {
        let mut output = String::from("[");
        let mut comma = "";
        for record in self.details.drain(..) {
            append(&mut output, comma, &record);
            comma = ",";
        }
        self.bytes = 0;
        for (&id, &(sequence, error)) in &self.losses {
            append(
                &mut output,
                comma,
                &Record {
                    id,
                    sequence,
                    action: 0,
                    path: Vec::new(),
                    error,
                    loss: true,
                },
            );
            comma = ",";
        }
        output.push(']');
        output
    }

    pub fn acknowledge(&mut self, id: usize, sequence: u64) {
        if self
            .losses
            .get(&id)
            .is_some_and(|&(current, _)| sequence >= current)
        {
            self.losses.remove(&id);
        }
    }
}

fn append(output: &mut String, comma: &str, record: &Record) {
    write!(
        output,
        "{comma}{{\"id\":{},\"sequence\":{},\"action\":{},\"path\":{},\"error\":{},\"loss\":{}}}",
        record.id,
        record.sequence,
        record.action,
        quote_utf16(&record.path),
        record.error,
        record.loss
    )
    .unwrap();
}

static QUEUE: OnceLock<Mutex<Queue>> = OnceLock::new();

pub fn queue() -> &'static Mutex<Queue> {
    QUEUE.get_or_init(|| Mutex::new(Queue::default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_loss_survives_drains_and_old_acknowledgements() {
        let mut queue = Queue::default();
        for sequence in 1..=4100 {
            queue.push(Record {
                id: 1,
                sequence,
                action: 1,
                path: vec![65],
                error: 0,
                loss: false,
            });
        }
        assert_eq!(queue.details.len(), 4096);
        assert!(queue.drain().contains("\"sequence\":4100"));
        queue.acknowledge(1, 4099);
        assert!(queue.drain().contains("\"loss\":true"));
        queue.acknowledge(1, 4100);
        assert_eq!(queue.drain(), "[]");
    }

    #[test]
    fn json_preserves_utf16_and_escapes_control_characters() {
        assert_eq!(
            quote_utf16(&[34, 92, 10, 0xd800]),
            "\"\\\"\\\\\\u000a\\ud800\""
        );
    }

    #[test]
    fn byte_cap_applies_before_record_count_and_preserves_loss() {
        let mut queue = Queue::default();
        for sequence in 1..=100 {
            let mut path = Vec::with_capacity(32768);
            path.push(65);
            queue.push(Record {
                id: 2,
                sequence,
                action: 1,
                path,
                error: 0,
                loss: false,
            });
        }
        assert!(queue.details.len() < 100);
        assert!(queue.bytes <= 4 * 1024 * 1024);
        assert_eq!(queue.losses.get(&2), Some(&(100, 0)));
        assert!(queue.drain().contains("\"loss\":true"));
        assert!(queue.drain().contains("\"sequence\":100"));
        queue.acknowledge(2, 100);
        assert_eq!(queue.drain(), "[]");
    }
}
