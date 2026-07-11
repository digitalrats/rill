//! Lua bindings for analyzer commands.
use std::sync::mpsc;

use mlua::{Lua, Result as LuaResult, Table};

use rill_telemetry::debug::protocol::AnalyzerCommand;

pub fn register(lua: &Lua, cmd_tx: mpsc::Sender<AnalyzerCommand>) -> LuaResult<Table> {
    let tbl = lua.create_table()?;

    let tx_set_breakpoint = cmd_tx.clone();
    tbl.set(
        "set_breakpoint",
        lua.create_function(move |_lua, (probe_id,): (u32,)| {
            let _ = tx_set_breakpoint.send(AnalyzerCommand::SetBreakpoint { probe_id });
            Ok(())
        })?,
    )?;

    let tx_clear_breakpoint = cmd_tx.clone();
    tbl.set(
        "clear_breakpoint",
        lua.create_function(move |_lua, (probe_id,): (u32,)| {
            let _ = tx_clear_breakpoint.send(AnalyzerCommand::ClearBreakpoint { probe_id });
            Ok(())
        })?,
    )?;

    let tx_continue = cmd_tx.clone();
    tbl.set(
        "continue_",
        lua.create_function(move |_lua, (): ()| {
            let _ = tx_continue.send(AnalyzerCommand::Continue);
            Ok(())
        })?,
    )?;

    let tx_step = cmd_tx.clone();
    tbl.set(
        "step",
        lua.create_function(move |_lua, (): ()| {
            let _ = tx_step.send(AnalyzerCommand::Step);
            Ok(())
        })?,
    )?;

    let tx_pause = cmd_tx.clone();
    tbl.set(
        "pause",
        lua.create_function(move |_lua, (): ()| {
            let _ = tx_pause.send(AnalyzerCommand::Pause);
            Ok(())
        })?,
    )?;

    let tx_get_value = cmd_tx.clone();
    tbl.set(
        "get_value",
        lua.create_function(move |_lua, (probe_id,): (u32,)| {
            let _ = tx_get_value.send(AnalyzerCommand::GetProbeValue { probe_id });
            Ok(())
        })?,
    )?;

    let tx_list_probes = cmd_tx.clone();
    tbl.set(
        "list_probes",
        lua.create_function(move |_lua, (): ()| {
            let _ = tx_list_probes.send(AnalyzerCommand::ListProbes);
            Ok(())
        })?,
    )?;

    let tx_list_nodes = cmd_tx.clone();
    tbl.set(
        "list_nodes",
        lua.create_function(move |_lua, (): ()| {
            let _ = tx_list_nodes.send(AnalyzerCommand::ListNodes);
            Ok(())
        })?,
    )?;

    let tx_enable = cmd_tx.clone();
    tbl.set(
        "enable_probe",
        lua.create_function(move |_lua, (probe_id,): (u32,)| {
            let _ = tx_enable.send(AnalyzerCommand::EnableProbe { probe_id });
            Ok(())
        })?,
    )?;

    let tx_disable = cmd_tx.clone();
    tbl.set(
        "disable_probe",
        lua.create_function(move |_lua, (probe_id,): (u32,)| {
            let _ = tx_disable.send(AnalyzerCommand::DisableProbe { probe_id });
            Ok(())
        })?,
    )?;

    let tx_values = cmd_tx.clone();
    tbl.set(
        "get_values",
        lua.create_function(move |_lua, (): ()| {
            let _ = tx_values.send(AnalyzerCommand::GetProbeValues);
            Ok(())
        })?,
    )?;

    Ok(tbl)
}
