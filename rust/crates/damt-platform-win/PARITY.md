# C# → Rust test parity — `damt-platform-win`

Oracle: `tests/Ocy.Platform.Windows.Tests/` (34 test methods). Rust tests live in `tests/pure.rs` and run on
every target; the Win32 implementations are exercised by `examples/probe.rs` on Windows.

| CsFile.Method | rust_fn |
| --- | --- |
| ActiveScreenAndStylerTests.RectMathTests.ToScreenRect_ConvertsLTRB | `to_screen_rect_converts_ltrb` |
| ActiveScreenAndStylerTests.RectMathTests.ToScreenRect_NegativeCoordinates_Work | `to_screen_rect_negative_coordinates_work` |
| ActiveScreenAndStylerTests.RectMathTests.ToScreenRect_AllNegative_Works | `to_screen_rect_all_negative_works` |
| ActiveScreenAndStylerTests.RectMathTests.ToScreenRect_ZeroSizedRect_Works | `to_screen_rect_zero_sized_rect_works` |
| ActiveScreenAndStylerTests.ExStyleTests.ClickThrough_KeepsExistingBits | `click_through_keeps_existing_bits` |
| ActiveScreenAndStylerTests.ExStyleTests.ClickThrough_AllFourFlagsSet | `click_through_all_four_flags_set` |
| ActiveScreenAndStylerTests.ExStyleTests.ClickThrough_Idempotent | `click_through_idempotent` |
| HelpersTests.MotionMathTests.Reduced_CallFailed_ReturnsFalse | `reduced_call_failed_returns_false` |
| HelpersTests.MotionMathTests.Reduced_ValueZero_ReturnsTrue | `reduced_value_zero_returns_true` |
| HelpersTests.MotionMathTests.Reduced_ValueNonZero_ReturnsFalse | `reduced_value_non_zero_returns_false` |
| HelpersTests.TaskbarThemeMathTests.FromRegistryValue_One_ReturnsTrue | `from_registry_value_one_returns_true` |
| HelpersTests.TaskbarThemeMathTests.FromRegistryValue_Zero_ReturnsFalse | `from_registry_value_zero_returns_false` |
| HelpersTests.TaskbarThemeMathTests.FromRegistryValue_Null_ReturnsNull | `from_registry_value_null_returns_null` |
| HelpersTests.TaskbarThemeMathTests.FromRegistryValue_OtherInt_ReturnsNull | `from_registry_value_other_int_returns_null` |
| HelpersTests.TaskbarThemeMathTests.FromRegistryValue_OtherType_ReturnsNull | `from_registry_value_other_type_returns_null` |
| HelpersTests.IdleMathTests.Compute_NowAfterLastInput_ReturnsDifference | `compute_now_after_last_input_returns_difference` |
| HelpersTests.IdleMathTests.Compute_SameTicks_ReturnsZero | `compute_same_ticks_returns_zero` |
| HelpersTests.IdleMathTests.Compute_Wraparound_IsSafe | `compute_wraparound_is_safe` |
| HelpersTests.IdleMathTests.Compute_MaxSpan_Wraparound_IsSafe | `compute_max_span_wraparound_is_safe` |
| HelpersTests.FocusStateMapperTests.Map_SuccessfulHresult_MapsEachState | `map_successful_hresult_maps_each_state` |
| HelpersTests.FocusStateMapperTests.Map_FailedHresult_ReturnsUnknown | `map_failed_hresult_returns_unknown` |
| HelpersTests.FocusStateMapperTests.Map_UnknownQuns_ReturnsUnknown | `map_unknown_quns_returns_unknown` |
| HelpersTests.AutostartCommandTests.Build_QuotesPath | `build_quotes_path` |
| HelpersTests.AutostartCommandTests.Build_SimplePath | `build_simple_path` |
| HelpersTests.AutostartCommandTests.Build_EmptyPath_Throws | `build_empty_path_errors` |
| HelpersTests.AutostartCommandTests.Build_NullPath_Throws | N/A: a Rust `&str` cannot be null; `build_empty_path_errors` covers the empty case |
| HelpersTests.AutostartCommandTests.Build_PathWithQuote_Throws | `build_path_with_quote_errors` |
| HelpersTests.SessionEventMapperTests.FromSessionSwitch_Lock_ReturnsLocked | `from_session_switch_lock_returns_locked` |
| HelpersTests.SessionEventMapperTests.FromSessionSwitch_Unlock_ReturnsUnlocked | `from_session_switch_unlock_returns_unlocked` |
| HelpersTests.SessionEventMapperTests.FromSessionSwitch_Logoff_ReturnsLoggingOff | `from_session_switch_logoff_returns_logging_off` |
| HelpersTests.SessionEventMapperTests.FromSessionSwitch_UnmappedReason_ReturnsNull | `from_session_switch_unmapped_reason_returns_none` |
| HelpersTests.SessionEventMapperTests.FromPowerMode_Resume_ReturnsResumed | `from_power_mode_resume_returns_resumed` |
| HelpersTests.SessionEventMapperTests.FromPowerMode_Suspend_ReturnsSuspending | `from_power_mode_suspend_returns_suspending` |
| HelpersTests.SessionEventMapperTests.FromPowerMode_UnmappedMode_ReturnsNull | `from_power_mode_unmapped_mode_returns_none` |

Ported: 33 of 34. N/A: `Build_NullPath_Throws` (no null `&str` in Rust). `WindowsWindowStyler` itself had no
separate tests; its `ExStyle` bit math is ported above, while the window mutation moves to the overlay crate (R2b).
