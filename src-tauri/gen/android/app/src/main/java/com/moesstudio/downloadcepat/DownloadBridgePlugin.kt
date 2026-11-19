package com.moesstudio.downloadcepat

import android.Manifest
import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.media.MediaScannerConnection
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.DocumentsContract
import android.provider.MediaStore
import android.provider.Settings
import android.webkit.MimeTypeMap
import android.webkit.WebView
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.core.content.FileProvider
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.security.MessageDigest
import java.util.UUID
import java.util.concurrent.Executors

@InvokeArg
internal class RegisterEventChannelArgs {
    lateinit var channel: Channel
}

@TauriPlugin
class DownloadBridgePlugin(private val activity: Activity) : Plugin(activity) {

    companion object {
        private const val EXTRA_INTENT_HANDLED = "com.moesstudio.downloadcepat.INTENT_HANDLED"
        private val lock = Any()
        private var eventChannel: Channel? = null
        private val pendingIntents = mutableListOf<Pair<String, String>>()
        private var notificationPermissionRequested = false
        private val ioExecutor = Executors.newCachedThreadPool()

        fun handleIncomingIntent(context: Context, intent: Intent?) {
            if (intent == null) return
            if (intent.getBooleanExtra(EXTRA_INTENT_HANDLED, false)) return
            val action = intent.action ?: return
            intent.putExtra(EXTRA_INTENT_HANDLED, true)
            var captured: Pair<String, String>? = null

            when (action) {
                Intent.ACTION_SEND -> {
                    val sharedText = intent.getStringExtra(Intent.EXTRA_TEXT)?.trim() ?: ""
                    val url = extractFirstSupportedUrl(sharedText)
                    if (url != null) {
                        val kind = if (url.startsWith("magnet:", ignoreCase = true)) "magnet" else "url"
                        captured = kind to url
                    }
                }

                Intent.ACTION_VIEW -> {
                    val data = intent.data
                    if (data != null) {
                        val scheme = data.scheme?.lowercase() ?: ""
                        when {
                            scheme == "magnet" -> {
                                captured = "magnet" to data.toString()
                            }
                            scheme == "content" || scheme == "file" -> {
                                val copiedPath = copyIncomingTorrentToCache(context, data)
                                if (copiedPath != null) {
                                    captured = "torrent_file" to copiedPath
                                }
                            }
                            else -> {
                                val raw = data.toString()
                                if (raw.startsWith("http://", true) || raw.startsWith("https://", true)) {
                                    captured = "url" to raw
                                }
                            }
                        }
                    }
                }
            }

            if (captured != null) {
                dispatchIncomingIntent(captured)
            }
        }

        private fun extractFirstSupportedUrl(text: String): String? {
            if (text.isBlank()) return null
            val magnetRegex = Regex("""magnet:\?[^\s<>"]+""", RegexOption.IGNORE_CASE)
            magnetRegex.find(text)?.value?.let { return it.trim() }

            val httpRegex = Regex("""https?://[^\s<>"]+""", RegexOption.IGNORE_CASE)
            httpRegex.find(text)?.value?.let { return it.trimEnd('.', ',', ';', ')', ']') }
            return null
        }

        private fun copyIncomingTorrentToCache(context: Context, uri: Uri): String? {
            return try {
                val dir = File(context.cacheDir, "incoming")
                if (!dir.exists()) dir.mkdirs()
                val outFile = File(dir, "intent_${UUID.randomUUID()}.torrent")
                context.contentResolver.openInputStream(uri)?.use { input ->
                    FileOutputStream(outFile).use { output ->
                        input.copyTo(output, 65536)
                    }
                }
                if (outFile.exists() && outFile.length() > 0) outFile.absolutePath else null
            } catch (_: Exception) {
                null
            }
        }

        private fun dispatchIncomingIntent(item: Pair<String, String>) {
            val ch: Channel?
            synchronized(lock) {
                ch = eventChannel
                if (ch == null) {
                    if (pendingIntents.none { it.first == item.first && it.second == item.second }) {
                        pendingIntents.add(item)
                    }
                }
            }
            if (ch != null) {
                val payload = JSObject().apply {
                    put("event", "incoming_intent")
                    put("kind", item.first)
                    put("value", item.second)
                }
                ch.send(payload)
            }
        }

        fun emitNotificationAction(actionName: String) {
            val ch = synchronized(lock) { eventChannel } ?: return
            val payload = JSObject().apply {
                put("event", "notification_action")
                put("action", actionName)
            }
            ch.send(payload)
        }
    }

    override fun load(webView: WebView) {
        super.load(webView)
        handleIncomingIntent(activity, activity.intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleIncomingIntent(activity, intent)
    }

    @Command
    fun registerEventChannel(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(RegisterEventChannelArgs::class.java)
            val buffered: List<Pair<String, String>>
            synchronized(lock) {
                eventChannel = args.channel
                buffered = pendingIntents.toList()
                pendingIntents.clear()
            }
            for (item in buffered) {
                args.channel.send(JSObject().apply {
                    put("event", "incoming_intent")
                    put("kind", item.first)
                    put("value", item.second)
                })
            }
            invoke.resolve(JSObject().apply { put("ok", true) })
        } catch (e: Exception) {
            invoke.reject("Failed to register event channel: ${e.message}")
        }
    }

    @Command
    fun consumePendingIntents(invoke: Invoke) {
        val drained: List<Pair<String, String>>
        synchronized(lock) {
            drained = pendingIntents.toList()
            pendingIntents.clear()
        }
        val arr = JSArray()
        for ((kind, value) in drained) {
            arr.put(JSObject().apply {
                put("kind", kind)
                put("value", value)
            })
        }
        invoke.resolve(JSObject().apply { put("intents", arr) })
    }

    @Command
    fun updateForegroundService(invoke: Invoke) {
        try {
            val args = invoke.getArgs()
            val activeCount = args.getInteger("activeCount", 0)
            val totalSpeedBps = args.optLong("totalSpeedBps", 0L)
            val progressPct = args.optDouble("progressPercent", 0.0)
            val title = args.getString("title", "Download Cepat") ?: "Download Cepat"

            if (activeCount > 0) {
                maybeRequestNotificationPermission()
            }

            DownloadForegroundService.startOrUpdate(
                activity.applicationContext,
                activeCount,
                totalSpeedBps,
                progressPct,
                title
            )
            invoke.resolve(JSObject().apply { put("ok", true) })
        } catch (e: Exception) {
            invoke.reject("Failed updating Foreground Service: ${e.message}")
        }
    }

    @Command
    fun exportToDownloads(invoke: Invoke) {
        val args = invoke.getArgs()
        val stagingPath = args.getString("stagingPath", "") ?: ""
        val relativeSubpath = args.getString("relativeSubpath", "") ?: ""
        val explicitMime = args.getString("mimeType", null)

        ioExecutor.execute {
            try {
                val srcFile = File(stagingPath)
                if (!srcFile.isFile) {
                    invoke.reject("Staging file does not exist: $stagingPath")
                    return@execute
                }

                val cleanSegments = relativeSubpath
                    .replace('\\', '/')
                    .split('/')
                    .filter { it.isNotBlank() && it != "." && it != ".." }
                val fileName = cleanSegments.lastOrNull() ?: srcFile.name
                val subDirSegments = if (cleanSegments.size > 1) {
                    cleanSegments.dropLast(1)
                } else {
                    emptyList()
                }
                val relDir = if (subDirSegments.isEmpty()) {
                    "Download Cepat"
                } else {
                    "Download Cepat/" + subDirSegments.joinToString("/")
                }
                val resolvedMime = resolveMimeType(fileName, explicitMime)

                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    val resolver = activity.contentResolver
                    val values = ContentValues().apply {
                        put(MediaStore.Downloads.DISPLAY_NAME, fileName)
                        put(MediaStore.Downloads.MIME_TYPE, resolvedMime)
                        put(
                            MediaStore.Downloads.RELATIVE_PATH,
                            "${Environment.DIRECTORY_DOWNLOADS}/$relDir"
                        )
                        put(MediaStore.Downloads.IS_PENDING, 1)
                    }
                    val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
                        ?: throw IllegalStateException("MediaStore insert returned null")

                    try {
                        resolver.openOutputStream(uri)?.use { out ->
                            FileInputStream(srcFile).use { input ->
                                input.copyTo(out, 65536)
                            }
                        } ?: throw IllegalStateException("Failed opening MediaStore output stream")

                        val completeValues = ContentValues().apply {
                            put(MediaStore.Downloads.IS_PENDING, 0)
                        }
                        resolver.update(uri, completeValues, null, null)
                    } catch (e: Exception) {
                        resolver.delete(uri, null, null)
                        throw e
                    }

                    val filePath = queryMediaStoreDataPath(uri)
                    val displayPath = "${Environment.DIRECTORY_DOWNLOADS}/$relDir/$fileName"
                    invoke.resolve(JSObject().apply {
                        put("contentUri", uri.toString())
                        if (filePath != null) put("filePath", filePath)
                        put("displayPath", displayPath)
                    })
                } else {
                    @Suppress("DEPRECATION")
                    val downloadsRoot =
                        Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS)
                    val targetDir = File(downloadsRoot, relDir)
                    if (!targetDir.exists()) targetDir.mkdirs()

                    val destFile = resolveUniqueFile(targetDir, fileName)
                    FileInputStream(srcFile).use { input ->
                        FileOutputStream(destFile).use { output ->
                            input.copyTo(output, 65536)
                        }
                    }

                    MediaScannerConnection.scanFile(
                        activity.applicationContext,
                        arrayOf(destFile.absolutePath),
                        arrayOf(resolvedMime),
                        null
                    )

                    val providerUri = FileProvider.getUriForFile(
                        activity,
                        "${activity.packageName}.fileprovider",
                        destFile
                    )
                    val displayPath = "${Environment.DIRECTORY_DOWNLOADS}/$relDir/${destFile.name}"
                    invoke.resolve(JSObject().apply {
                        put("contentUri", providerUri.toString())
                        put("filePath", destFile.absolutePath)
                        put("displayPath", displayPath)
                    })
                }
            } catch (e: Exception) {
                invoke.reject("Failed exporting to Downloads: ${e.message}")
            }
        }
    }

    @Command
    fun openFile(invoke: Invoke) {
        try {
            val args = invoke.getArgs()
            val pathOrUri = args.getString("path", "") ?: ""
            val folderMode = args.getBoolean("folderMode", false)

            if (folderMode) {
                val downloadsIntent = Intent(android.app.DownloadManager.ACTION_VIEW_DOWNLOADS).apply {
                    addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                }
                if (downloadsIntent.resolveActivity(activity.packageManager) != null) {
                    activity.startActivity(downloadsIntent)
                } else if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    val filesIntent = Intent(Intent.ACTION_VIEW).apply {
                        setDataAndType(
                            MediaStore.Downloads.EXTERNAL_CONTENT_URI,
                            DocumentsContract.Document.MIME_TYPE_DIR
                        )
                        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                    }
                    activity.startActivity(Intent.createChooser(filesIntent, "Open Downloads"))
                } else {
                    @Suppress("DEPRECATION")
                    val downloadsRoot =
                        Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS)
                    val targetDir =
                        File(downloadsRoot, "Download Cepat").takeIf { it.exists() } ?: downloadsRoot
                    val filesIntent = Intent(Intent.ACTION_VIEW).apply {
                        setDataAndType(
                            Uri.parse("content://com.android.externalstorage.documents/document/primary%3ADownload"),
                            DocumentsContract.Document.MIME_TYPE_DIR
                        )
                        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                    }
                    try {
                        activity.startActivity(Intent.createChooser(filesIntent, "Open Downloads"))
                    } catch (_: Exception) {
                        val fallbackIntent = Intent(Intent.ACTION_GET_CONTENT).apply {
                            type = "*/*"
                            addCategory(Intent.CATEGORY_OPENABLE)
                            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                        }
                        activity.startActivity(
                            Intent.createChooser(fallbackIntent, "Open Downloads (${targetDir.name})")
                        )
                    }
                }
                invoke.resolve(JSObject().apply {
                    put("message", "Downloads folder opened")
                })
                return
            }

            val (uri, mime) = resolveUriAndMime(pathOrUri)
                ?: throw IllegalStateException("File does not exist or URI is invalid: $pathOrUri")

            val viewIntent = Intent(Intent.ACTION_VIEW).apply {
                setDataAndType(uri, mime)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            activity.startActivity(Intent.createChooser(viewIntent, "Open file"))
            invoke.resolve(JSObject().apply {
                put("message", "File opened: $pathOrUri")
            })
        } catch (e: Exception) {
            invoke.reject("Failed opening file: ${e.message}")
        }
    }

    @Command
    fun shareFile(invoke: Invoke) {
        try {
            val args = invoke.getArgs()
            val pathOrUri = args.getString("path", "") ?: ""
            val (uri, mime) = resolveUriAndMime(pathOrUri)
                ?: throw IllegalStateException("File unavailable for sharing: $pathOrUri")

            val shareIntent = Intent(Intent.ACTION_SEND).apply {
                type = mime
                putExtra(Intent.EXTRA_STREAM, uri)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
            val chooser = Intent.createChooser(shareIntent, "Share via").apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            activity.startActivity(chooser)
            invoke.resolve(JSObject().apply {
                put("message", "Share dialog opened")
            })
        } catch (e: Exception) {
            invoke.reject("Failed sharing file: ${e.message}")
        }
    }

    @Command
    fun openExternalUrl(invoke: Invoke) {
        try {
            val url = invoke.getArgs().getString("url", "") ?: ""
            if (!(url.startsWith("http://", true) || url.startsWith("https://", true))) {
                invoke.reject("Invalid external URL")
                return
            }
            val intent = Intent(Intent.ACTION_VIEW, Uri.parse(url)).apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            activity.startActivity(intent)
            invoke.resolve(JSObject().apply {
                put("message", "Opened URL in browser: $url")
            })
        } catch (e: Exception) {
            invoke.reject("Failed opening external URL: ${e.message}")
        }
    }

    @Command
    fun installApk(invoke: Invoke) {
        try {
            val apkPath = invoke.getArgs().getString("apkPath", "") ?: ""
            val apkFile = File(apkPath)
            if (!apkFile.isFile) {
                invoke.reject("Update APK not found at: $apkPath")
                return
            }

            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O &&
                !activity.packageManager.canRequestPackageInstalls()
            ) {
                val permIntent = Intent(
                    Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                    Uri.parse("package:${activity.packageName}")
                ).apply {
                    addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                }
                activity.startActivity(permIntent)
                invoke.resolve(JSObject().apply {
                    put(
                        "message",
                        "Please allow 'Install unknown apps' permission in Android Settings, then tap Install Update again."
                    )
                })
                return
            }

            val apkUri = FileProvider.getUriForFile(
                activity,
                "${activity.packageName}.fileprovider",
                apkFile
            )
            val installIntent = Intent(Intent.ACTION_VIEW).apply {
                setDataAndType(apkUri, "application/vnd.android.package-archive")
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            activity.startActivity(installIntent)
            invoke.resolve(JSObject().apply {
                put("message", "APK installer opened. Confirm installation to update Download Cepat.")
            })
        } catch (e: Exception) {
            invoke.reject("Failed launching APK installer: ${e.message}")
        }
    }

    @Command
    fun computeChecksums(invoke: Invoke) {
        val pathOrUri = invoke.getArgs().getString("path", "") ?: ""
        ioExecutor.execute {
            try {
                val md5 = MessageDigest.getInstance("MD5")
                val sha1 = MessageDigest.getInstance("SHA-1")
                val sha256 = MessageDigest.getInstance("SHA-256")
                var totalBytes = 0L

                val stream = if (pathOrUri.startsWith("content://")) {
                    activity.contentResolver.openInputStream(Uri.parse(pathOrUri))
                } else {
                    val file = File(pathOrUri)
                    if (file.isFile) FileInputStream(file) else null
                } ?: throw IllegalStateException("Cannot open file for checksum: $pathOrUri")

                stream.use { input ->
                    val buf = ByteArray(65536)
                    while (true) {
                        val read = input.read(buf)
                        if (read <= 0) break
                        md5.update(buf, 0, read)
                        sha1.update(buf, 0, read)
                        sha256.update(buf, 0, read)
                        totalBytes += read.toLong()
                    }
                }

                invoke.resolve(JSObject().apply {
                    put("md5", md5.digest().toLowerHex())
                    put("sha1", sha1.digest().toLowerHex())
                    put("sha256", sha256.digest().toLowerHex())
                    put("sizeBytes", totalBytes)
                })
            } catch (e: Exception) {
                invoke.reject("Failed computing checksums: ${e.message}")
            }
        }
    }

    @Command
    fun readClipboardText(invoke: Invoke) {
        activity.runOnUiThread {
            try {
                val cm = activity.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
                val clip: ClipData? = cm?.primaryClip
                val text = if (clip != null && clip.itemCount > 0) {
                    clip.getItemAt(0).coerceToText(activity)?.toString()?.trim() ?: ""
                } else {
                    ""
                }
                invoke.resolve(JSObject().apply { put("text", text) })
            } catch (_: Exception) {
                invoke.resolve(JSObject().apply { put("text", "") })
            }
        }
    }

    private fun maybeRequestNotificationPermission() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU && !notificationPermissionRequested) {
            notificationPermissionRequested = true
            val granted = ContextCompat.checkSelfPermission(
                activity,
                Manifest.permission.POST_NOTIFICATIONS
            ) == PackageManager.PERMISSION_GRANTED
            if (!granted) {
                ActivityCompat.requestPermissions(
                    activity,
                    arrayOf(Manifest.permission.POST_NOTIFICATIONS),
                    1001
                )
            }
        }
    }

    private fun resolveMimeType(fileName: String, explicitMime: String?): String {
        val cleanedExplicit = explicitMime?.trim()?.takeIf { it.isNotEmpty() }
        if (cleanedExplicit != null && cleanedExplicit != "application/octet-stream") {
            return cleanedExplicit
        }
        val ext = fileName.substringAfterLast('.', "").lowercase()
        if (ext.isNotEmpty()) {
            MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext)?.let { return it }
            if (ext == "torrent") return "application/x-bittorrent"
            if (ext == "apk") return "application/vnd.android.package-archive"
        }
        return "application/octet-stream"
    }

    private fun resolveUriAndMime(pathOrUri: String): Pair<Uri, String>? {
        val trimmed = pathOrUri.trim()
        if (trimmed.isEmpty()) return null
        if (trimmed.startsWith("content://")) {
            val uri = Uri.parse(trimmed)
            val mime = activity.contentResolver.getType(uri) ?: "*/*"
            return uri to mime
        }
        val f = File(trimmed)
        if (!f.exists()) return null
        val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", f)
        val mime = resolveMimeType(f.name, null)
        return uri to mime
    }

    private fun queryMediaStoreDataPath(uri: Uri): String? {
        return try {
            @Suppress("DEPRECATION")
            val col = MediaStore.MediaColumns.DATA
            activity.contentResolver.query(uri, arrayOf(col), null, null, null)?.use { cursor ->
                if (cursor.moveToFirst()) {
                    val idx = cursor.getColumnIndex(col)
                    if (idx >= 0) cursor.getString(idx) else null
                } else {
                    null
                }
            }
        } catch (_: Exception) {
            null
        }
    }

    private fun resolveUniqueFile(dir: File, name: String): File {
        var candidate = File(dir, name)
        if (!candidate.exists()) return candidate
        val dotIdx = name.lastIndexOf('.')
        val stem = if (dotIdx > 0) name.substring(0, dotIdx) else name
        val ext = if (dotIdx > 0) name.substring(dotIdx) else ""
        for (i in 1..999) {
            candidate = File(dir, "$stem ($i)$ext")
            if (!candidate.exists()) return candidate
        }
        return File(dir, "${System.currentTimeMillis()}_$name")
    }

    private fun ByteArray.toLowerHex(): String {
        val sb = StringBuilder(size * 2)
        for (b in this) {
            sb.append(String.format("%02x", b))
        }
        return sb.toString()
    }
}
