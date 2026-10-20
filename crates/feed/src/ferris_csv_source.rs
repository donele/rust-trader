use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use core::{BookTicker, Incremental, Message, PacketBegin, PacketEnd, Side, SymbolId, TimeNanos};

use crate::MessageSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketDataCsvKind {
    BookL2,
    BookTicker,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MarketDataCsvError {
    Read(String),
    InvalidHeader {
        expected: Vec<&'static str>,
        found: String,
    },
    InvalidColumnCount {
        line: usize,
        expected: usize,
        found: usize,
    },
    InvalidField {
        line: usize,
        field: &'static str,
        value: String,
    },
    InvalidSide {
        line: usize,
        value: u8,
    },
}

#[derive(Debug, Clone, Copy)]
struct CsvRow {
    our_time: TimeNanos,
    exchange_time: u64,
    snapshot: bool,
    message: Message,
}

#[derive(Debug, Clone, Copy)]
struct PacketState {
    our_time: TimeNanos,
    exchange_time: u64,
    snapshot: bool,
    pending_row: Option<CsvRow>,
}

#[derive(Debug)]
pub struct MarketDataCsvSource {
    reader: BufReader<File>,
    name: String,
    kind: MarketDataCsvKind,
    symbol: SymbolId,
    price_increment: f64,
    qty_increment: f64,
    line_number: usize,
    lookahead: Option<CsvRow>,
    packet: Option<PacketState>,
    pending: VecDeque<Message>,
    error: Option<MarketDataCsvError>,
}

impl MarketDataCsvSource {
    pub fn from_path(
        path: impl AsRef<Path>,
        kind: MarketDataCsvKind,
        symbol: SymbolId,
        price_increment: f64,
        qty_increment: f64,
    ) -> Result<Self, MarketDataCsvError> {
        let path = path.as_ref();
        let file = File::open(path).map_err(|error| MarketDataCsvError::Read(error.to_string()))?;
        let mut source = Self {
            reader: BufReader::new(file),
            name: path.to_string_lossy().into_owned(),
            kind,
            symbol,
            price_increment,
            qty_increment,
            line_number: 0,
            lookahead: None,
            packet: None,
            pending: VecDeque::new(),
            error: None,
        };
        source.validate_header()?;
        Ok(source)
    }

    pub fn try_next_message(&mut self) -> Result<Option<Message>, MarketDataCsvError> {
        if let Some(message) = self.pending.pop_front() {
            return Ok(Some(message));
        }

        if let Some(mut packet) = self.packet {
            if let Some(row) = packet.pending_row.take() {
                self.packet = Some(packet);
                return Ok(Some(row.message));
            }

            match self.read_row()? {
                Some(row) if same_packet(&packet, &row) => {
                    packet.exchange_time = row.exchange_time;
                    self.packet = Some(packet);
                    Ok(Some(row.message))
                }
                Some(row) => {
                    self.lookahead = Some(row);
                    self.packet = None;
                    Ok(Some(Message::PacketEnd(PacketEnd {
                        symbol: self.symbol,
                        ts_nanos: packet.our_time,
                        snapshot: packet.snapshot,
                    })))
                }
                None => {
                    self.packet = None;
                    Ok(Some(Message::PacketEnd(PacketEnd {
                        symbol: self.symbol,
                        ts_nanos: packet.our_time,
                        snapshot: packet.snapshot,
                    })))
                }
            }
        } else {
            let row = if let Some(row) = self.lookahead.take() {
                row
            } else {
                match self.read_row()? {
                    Some(row) => row,
                    None => return Ok(None),
                }
            };
            self.packet = Some(PacketState {
                our_time: row.our_time,
                exchange_time: row.exchange_time,
                snapshot: row.snapshot,
                pending_row: Some(row),
            });
            Ok(Some(Message::PacketBegin(PacketBegin {
                symbol: self.symbol,
                ts_nanos: row.our_time,
                snapshot: row.snapshot,
            })))
        }
    }

    pub fn error(&self) -> Option<&MarketDataCsvError> {
        self.error.as_ref()
    }

    fn validate_header(&mut self) -> Result<(), MarketDataCsvError> {
        let mut line = String::new();
        let bytes = self
            .reader
            .read_line(&mut line)
            .map_err(|error| MarketDataCsvError::Read(error.to_string()))?;
        if bytes == 0 {
            return Err(MarketDataCsvError::InvalidHeader {
                expected: expected_header(self.kind).to_vec(),
                found: String::new(),
            });
        }
        self.line_number = 1;
        let found = line.trim_end_matches(['\r', '\n']);
        let expected = expected_header(self.kind);
        let columns: Vec<_> = found.split(',').map(str::trim).collect();
        if columns != expected {
            return Err(MarketDataCsvError::InvalidHeader {
                expected: expected.to_vec(),
                found: found.to_owned(),
            });
        }
        Ok(())
    }

    fn read_row(&mut self) -> Result<Option<CsvRow>, MarketDataCsvError> {
        let mut line = String::new();
        loop {
            line.clear();
            let bytes = self
                .reader
                .read_line(&mut line)
                .map_err(|error| MarketDataCsvError::Read(error.to_string()))?;
            if bytes == 0 {
                return Ok(None);
            }
            self.line_number += 1;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            return self.parse_row(trimmed).map(Some);
        }
    }

    fn parse_row(&self, line: &str) -> Result<CsvRow, MarketDataCsvError> {
        let columns: Vec<_> = line.split(',').map(str::trim).collect();
        let expected = expected_header(self.kind);
        if columns.len() != expected.len() {
            return Err(MarketDataCsvError::InvalidColumnCount {
                line: self.line_number,
                expected: expected.len(),
                found: columns.len(),
            });
        }

        let our_time = parse_i64(columns[0], self.line_number, "our_time")?;
        let exchange_time = match self.kind {
            MarketDataCsvKind::BookL2 => parse_u64(columns[2], self.line_number, "exchange_time")?,
            MarketDataCsvKind::BookTicker => {
                parse_u64(columns[1], self.line_number, "exchange_time")?
            }
        };

        match self.kind {
            MarketDataCsvKind::BookL2 => {
                let snapshot = parse_u8(columns[1], self.line_number, "is_snapshot")? != 0;
                let price = parse_f64(columns[3], self.line_number, "price")?;
                let qty = parse_f64(columns[4], self.line_number, "qty")?;
                let side_value = parse_u8(columns[5], self.line_number, "side")?;
                let side = match side_value {
                    0 => Side::Buy,
                    1 => Side::Sell,
                    value => {
                        return Err(MarketDataCsvError::InvalidSide {
                            line: self.line_number,
                            value,
                        })
                    }
                };
                Ok(CsvRow {
                    our_time,
                    exchange_time,
                    snapshot,
                    message: Message::Incremental(Incremental {
                        ts_nanos: our_time,
                        symbol: self.symbol,
                        price: normalize(price, self.price_increment),
                        qty: normalize(qty, self.qty_increment),
                        num_orders_at_level: 0,
                        side,
                    }),
                })
            }
            MarketDataCsvKind::BookTicker => Ok(CsvRow {
                our_time,
                exchange_time,
                snapshot: false,
                message: Message::BookTicker(BookTicker {
                    ts_nanos: our_time,
                    symbol: self.symbol,
                    best_bid_price: normalize(
                        parse_f64(columns[2], self.line_number, "bid_price")?,
                        self.price_increment,
                    ),
                    best_bid_qty: normalize(
                        parse_f64(columns[3], self.line_number, "bid_qty")?,
                        self.qty_increment,
                    ),
                    best_ask_price: normalize(
                        parse_f64(columns[4], self.line_number, "ask_price")?,
                        self.price_increment,
                    ),
                    best_ask_qty: normalize(
                        parse_f64(columns[5], self.line_number, "ask_qty")?,
                        self.qty_increment,
                    ),
                    update_id: exchange_time,
                }),
            }),
        }
    }
}

impl MessageSource for MarketDataCsvSource {
    fn next_message(&mut self) -> Option<Message> {
        match self.try_next_message() {
            Ok(message) => message,
            Err(error) => {
                self.error = Some(error);
                None
            }
        }
    }

    fn source_name(&self) -> &str {
        &self.name
    }
}

fn expected_header(kind: MarketDataCsvKind) -> &'static [&'static str] {
    match kind {
        MarketDataCsvKind::BookL2 => &[
            "our_time",
            "is_snapshot",
            "exchange_time",
            "price",
            "qty",
            "side",
            "flags",
        ],
        MarketDataCsvKind::BookTicker => &[
            "our_time",
            "exchange_time",
            "bid_price",
            "bid_qty",
            "ask_price",
            "ask_qty",
        ],
    }
}

fn same_packet(packet: &PacketState, row: &CsvRow) -> bool {
    packet.our_time == row.our_time && packet.snapshot == row.snapshot
}

fn normalize(value: f64, increment: f64) -> f64 {
    (value / increment).round() * increment
}

fn parse_i64(value: &str, line: usize, field: &'static str) -> Result<i64, MarketDataCsvError> {
    value.parse().map_err(|_| MarketDataCsvError::InvalidField {
        line,
        field,
        value: value.into(),
    })
}

fn parse_u8(value: &str, line: usize, field: &'static str) -> Result<u8, MarketDataCsvError> {
    value.parse().map_err(|_| MarketDataCsvError::InvalidField {
        line,
        field,
        value: value.into(),
    })
}

fn parse_u64(value: &str, line: usize, field: &'static str) -> Result<u64, MarketDataCsvError> {
    value.parse().map_err(|_| MarketDataCsvError::InvalidField {
        line,
        field,
        value: value.into(),
    })
}

fn parse_f64(value: &str, line: usize, field: &'static str) -> Result<f64, MarketDataCsvError> {
    value.parse().map_err(|_| MarketDataCsvError::InvalidField {
        line,
        field,
        value: value.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn reads_l2_rows_and_packet_boundaries() {
        let path = std::env::temp_dir().join("rust_trade_sim_book_l2.csv");
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(
            file,
            "our_time,is_snapshot,exchange_time,price,qty,side,flags"
        )
        .unwrap();
        writeln!(file, "10,1,100,100.03,2.0,0,0").unwrap();
        writeln!(file, "10,1,100,100.02,3.0,1,0").unwrap();
        writeln!(file, "20,0,200,100.04,1.0,0,0").unwrap();
        drop(file);

        let mut source = MarketDataCsvSource::from_path(
            &path,
            MarketDataCsvKind::BookL2,
            SymbolId(7),
            0.01,
            0.1,
        )
        .unwrap();
        assert!(matches!(
            source.try_next_message().unwrap(),
            Some(Message::PacketBegin(PacketBegin { snapshot: true, .. }))
        ));
        assert!(matches!(
            source.try_next_message().unwrap(),
            Some(Message::Incremental(Incremental {
                side: Side::Buy,
                ..
            }))
        ));
        assert!(matches!(
            source.try_next_message().unwrap(),
            Some(Message::Incremental(Incremental {
                side: Side::Sell,
                ..
            }))
        ));
        assert!(matches!(
            source.try_next_message().unwrap(),
            Some(Message::PacketEnd(PacketEnd { snapshot: true, .. }))
        ));
        assert!(matches!(
            source.try_next_message().unwrap(),
            Some(Message::PacketBegin(PacketBegin {
                snapshot: false,
                ..
            }))
        ));
        assert!(matches!(
            source.try_next_message().unwrap(),
            Some(Message::Incremental(_))
        ));
        assert!(matches!(
            source.try_next_message().unwrap(),
            Some(Message::PacketEnd(_))
        ));
        assert!(source.try_next_message().unwrap().is_none());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn reads_book_ticker_rows() {
        let path = std::env::temp_dir().join("rust_trade_sim_book_ticker.csv");
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(
            file,
            "our_time,exchange_time,bid_price,bid_qty,ask_price,ask_qty"
        )
        .unwrap();
        writeln!(file, "10,100,100.03,2.04,100.04,3.01").unwrap();
        drop(file);

        let mut source = MarketDataCsvSource::from_path(
            &path,
            MarketDataCsvKind::BookTicker,
            SymbolId(7),
            0.01,
            0.1,
        )
        .unwrap();
        assert!(matches!(
            source.try_next_message().unwrap(),
            Some(Message::PacketBegin(_))
        ));
        assert!(matches!(
            source.try_next_message().unwrap(),
            Some(Message::BookTicker(BookTicker { update_id: 100, .. }))
        ));
        assert!(matches!(
            source.try_next_message().unwrap(),
            Some(Message::PacketEnd(_))
        ));
        assert!(source.try_next_message().unwrap().is_none());
        std::fs::remove_file(path).unwrap();
    }
}
