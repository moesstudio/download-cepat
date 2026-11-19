package com.moesstudio.downloadcepat

import android.content.Intent
import android.os.Bundle
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        intent?.let { DownloadBridgePlugin.handleIncomingIntent(this, it) }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        DownloadBridgePlugin.handleIncomingIntent(this, intent)
    }
}
