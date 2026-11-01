//! What the player did: the code of each level, the levels solved, the hints seen.
use std::collections::{BTreeMap, BTreeSet};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::storage;

#[derive(Resource, Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Progress {
    /// The code in the editor, per level key; `sandbox` for the sandbox.
    pub code: BTreeMap<String, String>,
    pub solved: BTreeSet<String>,
    /// How many hints of each level are shown.
    pub hints: BTreeMap<String, usize>,
}

impl Progress {
    pub fn load() -> Self {
        storage::load("progress").unwrap_or_default()
    }

    pub fn save(&self) {
        storage::save("progress", self);
    }
}
