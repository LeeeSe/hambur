package com.hambur.chat.reducer

import android.util.Log
import com.hambur.chat.uniffi.AppBootstrapConfig
import com.hambur.chat.uniffi.createRuntime
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class AppShellState(
    val runtimeStatus: String = "Starting",
    val latestEventKind: String = "Waiting",
    val footer: String = "Rust runtime owns backend state",
)

class HamburUiStore(appFilesDir: String) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val runtime = createRuntime(AppBootstrapConfig(appFilesDir = appFilesDir))
    private val _state = MutableStateFlow(AppShellState())

    val state: StateFlow<AppShellState> = _state.asStateFlow()

    init {
        scope.launch {
            val event = runtime.nextEvent()
            Log.i(
                "HamburBackend",
                "Collected backend event kind=${event?.kind ?: "None"} sequence=${event?.sequence ?: 0UL}",
            )
            _state.update {
                it.copy(
                    runtimeStatus = if (event != null) "Ready" else "Closed",
                    latestEventKind = event?.kind ?: "None",
                )
            }
        }
    }

    fun shutdown() {
        runtime.shutdown()
        scope.cancel()
    }
}
