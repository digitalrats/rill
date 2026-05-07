// PatchbayEngine removed.
//
// All functionality was moved to PatchbayControl:
//   - tokio runtime check   → PatchbayControl::new()
//   - Drop::stop_all         → impl Drop for PatchbayControl  
//   - add_lfo / add_envelope → add_lfo_async / add_envelope_async
//   - add_automaton          → add_automaton_task
//   - load_document          → PatchbayDocument::apply_to_async() directly
//   - stop()                 → PatchbayControl::stop_all()
