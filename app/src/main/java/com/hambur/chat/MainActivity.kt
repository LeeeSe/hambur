package com.hambur.chat

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import com.hambur.chat.platform.AndroidPlatformAdapter
import com.hambur.chat.ui.AppShell

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val platformAdapter = AndroidPlatformAdapter(applicationContext)

        setContent {
            AppShell(
                appFilesDir = filesDir.absolutePath,
                platformAdapter = platformAdapter,
            )
        }
    }
}
