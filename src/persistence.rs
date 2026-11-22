// Copyright (C) 2026 CRV Science Network (https://crvscience.com)
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://gnu.org>.

#![forbid(unsafe_code)]

use crate::agents::{CanopyLidarDrone, SpeciesPopulation, StewardCrew};
use crate::ecology::{ClimateRegime, TileState};
use crate::strategies::{BudgetLedger, StrategyPortfolio};
use rusqlite::{params, Connection, Result as SqlResult};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationSaveState {
    pub schema_version: String,
    pub save_name: String,
    pub seed: u64,
    pub day_of_year: f32,
    pub climate_regime: ClimateRegime,
    pub ledger: BudgetLedger,
    pub portfolio: StrategyPortfolio,
    pub tiles: Vec<TileState>,
    pub populations: Vec<SpeciesPopulation>,
    pub crews: Vec<StewardCrew>,
    pub drones: Vec<CanopyLidarDrone>,
}

impl SimulationSaveState {
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json_str(raw: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(raw)
    }
}

pub struct SaveDatabase {
    conn: Connection,
}

impl SaveDatabase {
    pub fn open_wal<P: AsRef<Path>>(path: P) -> SqlResult<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS save_slots (
                slot_name TEXT PRIMARY KEY,
                year INTEGER NOT NULL,
                eri REAL NOT NULL,
                payload_json TEXT NOT NULL,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP
            );",
        )?;
        Ok(Self { conn })
    }

    pub fn open_in_memory() -> SqlResult<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS save_slots (
                slot_name TEXT PRIMARY KEY,
                year INTEGER NOT NULL,
                eri REAL NOT NULL,
                payload_json TEXT NOT NULL,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP
            );",
        )?;
        Ok(Self { conn })
    }

    pub fn save_slot(&self, slot_name: &str, eri: f32, state: &SimulationSaveState) -> SqlResult<()> {
        let payload = serde_json::to_string(state).unwrap_or_default();
        self.conn.execute(
            "INSERT INTO save_slots (slot_name, year, eri, payload_json, updated_at)
             VALUES (?1, ?2, ?3, ?4, CURRENT_TIMESTAMP)
             ON CONFLICT(slot_name) DO UPDATE SET
                year = excluded.year,
                eri = excluded.eri,
                payload_json = excluded.payload_json,
                updated_at = CURRENT_TIMESTAMP;",
            params![slot_name, state.ledger.current_year, eri, payload],
        )?;
        Ok(())
    }

    pub fn load_slot(&self, slot_name: &str) -> SqlResult<Option<SimulationSaveState>> {
        let mut stmt = self
            .conn
            .prepare("SELECT payload_json FROM save_slots WHERE slot_name = ?1")?;
        let mut rows = stmt.query(params![slot_name])?;
        if let Some(row) = rows.next()? {
            let raw: String = row.get(0)?;
            let parsed = serde_json::from_str(&raw).ok();
            Ok(parsed)
        } else {
            Ok(None)
        }
    }

    pub fn list_slots(&self) -> SqlResult<Vec<(String, u32, f32)>> {
        let mut stmt = self.conn.prepare(
            "SELECT slot_name, year, eri FROM save_slots ORDER BY updated_at DESC",
        )?;
        let iter = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        let mut out = Vec::new();
        for item in iter {
            out.push(item?);
        }
        Ok(out)
    }
}
