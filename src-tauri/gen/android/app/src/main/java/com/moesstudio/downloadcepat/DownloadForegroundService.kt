package com.moesstudio.downloadcepat

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import java.util.Locale

class DownloadForegroundService : Service() {

    companion object {
        const val CHANNEL_ID = "dc_downloads"
        const val NOTIFICATION_ID = 4201
        const val ACTION_UPDATE = "com.moesstudio.downloadcepat.ACTION_FGS_UPDATE"
        const val ACTION_STOP = "com.moesstudio.downloadcepat.ACTION_FGS_STOP"
        const val ACTION_PAUSE_ALL = "com.moesstudio.downloadcepat.ACTION_FGS_PAUSE_ALL"

        const val EXTRA_ACTIVE_COUNT = "activeCount"
        const val EXTRA_SPEED_BPS = "totalSpeedBps"
        const val EXTRA_PROGRESS_PCT = "progressPercent"
        const val EXTRA_TITLE = "title"

        fun startOrUpdate(
            context: Context,
            activeCount: Int,
            totalSpeedBps: Long,
            progressPercent: Double,
            title: String
        ) {
            if (activeCount <= 0) {
                stop(context)
                return
            }
            val intent = Intent(context, DownloadForegroundService::class.java).apply {
                action = ACTION_UPDATE
                putExtra(EXTRA_ACTIVE_COUNT, activeCount)
                putExtra(EXTRA_SPEED_BPS, totalSpeedBps)
                putExtra(EXTRA_PROGRESS_PCT, progressPercent)
                putExtra(EXTRA_TITLE, title)
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
        }

        fun stop(context: Context) {
            val intent = Intent(context, DownloadForegroundService::class.java).apply {
                action = ACTION_STOP
            }
            try {
                context.startService(intent)
            } catch (_: Exception) {
                context.stopService(Intent(context, DownloadForegroundService::class.java))
            }
        }
    }

    private var wakeLock: PowerManager.WakeLock? = null
    private var isForegroundStarted = false

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        ensureNotificationChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> {
                releaseWakeLock()
                ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
                isForegroundStarted = false
                stopSelf()
                return START_NOT_STICKY
            }

            ACTION_PAUSE_ALL -> {
                DownloadBridgePlugin.emitNotificationAction("pause_all")
                releaseWakeLock()
                ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
                isForegroundStarted = false
                stopSelf()
                return START_NOT_STICKY
            }

            else -> {
                val activeCount = intent?.getIntExtra(EXTRA_ACTIVE_COUNT, 1) ?: 1
                if (activeCount <= 0) {
                    releaseWakeLock()
                    ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
                    isForegroundStarted = false
                    stopSelf()
                    return START_NOT_STICKY
                }
                val speedBps = intent?.getLongExtra(EXTRA_SPEED_BPS, 0L) ?: 0L
                val progressPct = (intent?.getDoubleExtra(EXTRA_PROGRESS_PCT, 0.0) ?: 0.0).coerceIn(0.0, 100.0)
                val rawTitle = intent?.getStringExtra(EXTRA_TITLE)?.takeIf { it.isNotBlank() }
                    ?: "Download Cepat"

                acquireWakeLock()
                val notification = buildNotification(activeCount, speedBps, progressPct, rawTitle)
                if (!isForegroundStarted) {
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                        ServiceCompat.startForeground(
                            this,
                            NOTIFICATION_ID,
                            notification,
                            ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC
                        )
                    } else {
                        startForeground(NOTIFICATION_ID, notification)
                    }
                    isForegroundStarted = true
                } else {
                    val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
                    manager.notify(NOTIFICATION_ID, notification)
                }
                return START_STICKY
            }
        }
    }

    override fun onDestroy() {
        releaseWakeLock()
        isForegroundStarted = false
        super.onDestroy()
    }

    private fun ensureNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            val existing = manager.getNotificationChannel(CHANNEL_ID)
            if (existing == null) {
                val channel = NotificationChannel(
                    CHANNEL_ID,
                    "Active Downloads",
                    NotificationManager.IMPORTANCE_LOW
                ).apply {
                    description = "Shows multi-thread download progress and speed"
                    setShowBadge(false)
                }
                manager.createNotificationChannel(channel)
            }
        }
    }

    private fun acquireWakeLock() {
        val pm = getSystemService(Context.POWER_SERVICE) as? PowerManager ?: return
        if (wakeLock == null) {
            wakeLock = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "DownloadCepat::ActiveDownload").apply {
                setReferenceCounted(false)
            }
        }
        wakeLock?.acquire(60 * 60 * 1000L) // refresh 1h safety timeout; released cleanly on stop/destroy
    }

    private fun releaseWakeLock() {
        try {
            if (wakeLock?.isHeld == true) {
                wakeLock?.release()
            }
        } catch (_: Exception) {
        } finally {
            wakeLock = null
        }
    }

    private fun buildNotification(
        activeCount: Int,
        speedBps: Long,
        progressPct: Double,
        title: String
    ): Notification {
        val openIntent = Intent(this, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
        }
        val openPending = PendingIntent.getActivity(
            this,
            0,
            openIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        val pauseIntent = Intent(this, DownloadForegroundService::class.java).apply {
            action = ACTION_PAUSE_ALL
        }
        val pausePending = PendingIntent.getService(
            this,
            1,
            pauseIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        val pctInt = progressPct.toInt().coerceIn(0, 100)
        val speedText = formatSpeed(speedBps)
        val headerTitle = if (activeCount == 1) title else "$activeCount downloads active"
        val statusText = "$pctInt% • $speedText"

        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle(headerTitle)
            .setContentText(statusText)
            .setSmallIcon(R.mipmap.ic_launcher)
            .setContentIntent(openPending)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setSilent(true)
            .setProgress(100, pctInt, pctInt <= 0)
            .addAction(android.R.drawable.ic_media_pause, "Pause All", pausePending)
            .build()
    }

    private fun formatSpeed(bps: Long): String {
        if (bps <= 0L) return "0 KB/s"
        val mb = 1024.0 * 1024.0
        val kb = 1024.0
        return when {
            bps >= mb -> String.format(Locale.US, "%.2f MB/s", bps / mb)
            else -> String.format(Locale.US, "%.1f KB/s", bps / kb)
        }
    }
}
