// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Python bindings from `pyo3`.

pub mod fix;
pub mod http;
pub mod websocket;

use pyo3::prelude::*;

/// Loaded as `nautilus_pyo3.coinbase`.
#[pymodule]
/// # Errors
///
/// Returns a Python exception if module initialization fails.
pub fn coinbase_intx(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<super::http::CoinbaseIntxHttpClient>()?;
    m.add_class::<super::websocket::CoinbaseIntxWebSocketClient>()?;
    m.add_class::<super::fix::CoinbaseIntxFixClient>()?;
    Ok(())
}
