package io.github.Yuu518.singboard

import android.app.Activity
import android.content.Intent
import android.content.pm.PackageInfo
import android.content.pm.PackageManager
import android.graphics.Color
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.OpenableColumns
import android.provider.Settings
import androidx.core.content.FileProvider
import androidx.core.view.WindowCompat
import android.util.AtomicFile
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import java.io.File
import java.nio.charset.CharacterCodingException
import java.security.MessageDigest
import java.util.concurrent.Executors

class MainActivity : FlutterActivity() {
    private val worker = Executors.newSingleThreadExecutor()
    private var picker: MethodChannel.Result? = null
    private var channel: MethodChannel? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        WindowCompat.setDecorFitsSystemWindows(window, false)
        @Suppress("DEPRECATION")
        run {
            window.statusBarColor = Color.TRANSPARENT
            window.navigationBarColor = Color.TRANSPARENT
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            window.isNavigationBarContrastEnforced = false
        }
        worker.execute { cleanupStaleFiles(null) }
    }

    private fun cleanupStaleFiles(keepUpdateDir: File?) {
        runCatching {
            val updates = File(filesDir, "updates")
            updates.listFiles()?.forEach { child ->
                if (keepUpdateDir != null && child.canonicalFile == keepUpdateDir.canonicalFile) return@forEach
                child.deleteRecursively()
            }
            val backups = filesDir.listFiles { file ->
                file.isFile && file.name.startsWith("document-") && file.name.endsWith(".bak")
            }?.sortedByDescending { it.lastModified() } ?: emptyList()
            val cutoff = System.currentTimeMillis() - 7L * 24 * 60 * 60 * 1000
            backups.forEachIndexed { index, file ->
                if (index >= 5 || file.lastModified() < cutoff) {
                    file.delete()
                }
            }
        }
    }

    override fun configureFlutterEngine(engine: FlutterEngine) {
        super.configureFlutterEngine(engine)
        val methodChannel = MethodChannel(engine.dartExecutor.binaryMessenger, "singboard/config")
        channel = methodChannel
        methodChannel.setMethodCallHandler { call, result ->
            when (call.method) {
                "directory" -> result.success(filesDir.absolutePath)
                "version" -> result.success(packageManager.getPackageInfo(packageName, 0).versionName)
                "installedApk" -> result.success(applicationInfo.sourceDir)
                "openChangelog" -> {
                    try {
                        startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("https://github.com/Yuu518/singboard/releases")))
                        result.success(null)
                    } catch (e: Exception) { result.error("changelog", e.localizedMessage, null) }
                }
                "installUpdate" -> installUpdate(call, result)
                "pick" -> {
                    if (picker != null) {
                        result.error("busy", "文件选择器已打开", null)
                    } else {
                        picker = result
                        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE)
                        intent.type = "*/*"
                        try { startActivityForResult(intent, 9301) }
                        catch (e: Exception) { picker = null; result.error("open", e.localizedMessage, null) }
                    }
                }
                "pickImage" -> {
                    if (picker != null) {
                        result.error("busy", "文件选择器已打开", null)
                    } else {
                        picker = result
                        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE)
                        intent.type = "image/*"
                        try { startActivityForResult(intent, 9301) }
                        catch (e: Exception) { picker = null; result.error("open", e.localizedMessage, null) }
                    }
                }
                "wallpaperPath" -> {
                    val file = File(filesDir, "wallpaper.png")
                    result.success(if (file.isFile) file.absolutePath else null)
                }
                "clearWallpaper" -> perform(result) {
                    val file = File(filesDir, "wallpaper.png")
                    if (file.exists()) file.delete()
                    true
                }
                "readImageBytes" -> perform(result) {
                    val uri = Uri.parse(requireNotNull(call.argument<String>("uri")))
                    require(uri.scheme == "content") { "需要系统图片选择器返回的 URI" }
                    val input = contentResolver.openInputStream(uri) ?: error("无法读取图片")
                    input.use { stream ->
                        val output = java.io.ByteArrayOutputStream()
                        val buffer = ByteArray(16384)
                        while (true) {
                            val n = stream.read(buffer)
                            if (n < 0) break
                            require(output.size() + n <= 24 * 1024 * 1024) { "图片超过 24 MiB" }
                            output.write(buffer, 0, n)
                        }
                        output.toByteArray()
                    }
                }
                "saveWallpaper" -> perform(result) {
                    val bytes = requireNotNull(call.argument<ByteArray>("bytes"))
                    require(bytes.isNotEmpty() && bytes.size <= 24 * 1024 * 1024) { "壁纸数据大小无效" }
                    val target = AtomicFile(File(filesDir, "wallpaper.png"))
                    val out = target.startWrite()
                    try {
                        out.write(bytes)
                        target.finishWrite(out)
                    } catch (e: Exception) {
                        target.failWrite(out)
                        throw e
                    }
                    target.baseFile.absolutePath
                }
                "read", "save", "backup" -> perform(result) { document(call) }
                else -> result.notImplemented()
            }
        }
    }

    private fun signingCertificates(info: PackageInfo): Set<String> {
        val signatures = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            val signingInfo = info.signingInfo
            if (signingInfo == null) {
                emptyArray()
            } else if (signingInfo.hasMultipleSigners()) {
                signingInfo.apkContentsSigners ?: emptyArray()
            } else {
                signingInfo.signingCertificateHistory ?: signingInfo.apkContentsSigners ?: emptyArray()
            }
        } else {
            @Suppress("DEPRECATION")
            info.signatures ?: emptyArray()
        }
        return signatures.map { sig ->
            MessageDigest.getInstance("SHA-256")
                .digest(sig.toByteArray())
                .joinToString("") { "%02x".format(it) }
        }.toSet()
    }

    private fun versionCodeOf(info: PackageInfo): Long {
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            info.longVersionCode
        } else {
            @Suppress("DEPRECATION")
            info.versionCode.toLong()
        }
    }

    private fun installUpdate(call: MethodCall, result: MethodChannel.Result) {
        val path = call.argument<String>("path")
        worker.execute {
            try {
                val apk = File(requireNotNull(path)).canonicalFile
                require(apk.isFile && apk.parentFile?.parentFile == File(filesDir, "updates").canonicalFile) { "无效的安装包路径" }
                val sha = MessageDigest.getInstance("SHA-256")
                apk.inputStream().use { input ->
                    val buffer = ByteArray(65536)
                    while (true) {
                        val count = input.read(buffer)
                        if (count < 0) break
                        sha.update(buffer, 0, count)
                    }
                }
                require(sha.digest().joinToString("") { "%02x".format(it) } == apk.parentFile?.name) { "安装包在下载后发生变化" }
                val flags = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                    PackageManager.GET_SIGNING_CERTIFICATES
                } else {
                    @Suppress("DEPRECATION")
                    PackageManager.GET_SIGNATURES
                }
                val archive = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    packageManager.getPackageArchiveInfo(apk.path, PackageManager.PackageInfoFlags.of(flags.toLong()))
                } else {
                    @Suppress("DEPRECATION")
                    packageManager.getPackageArchiveInfo(apk.path, flags)
                }
                val current = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    packageManager.getPackageInfo(packageName, PackageManager.PackageInfoFlags.of(flags.toLong()))
                } else {
                    @Suppress("DEPRECATION")
                    packageManager.getPackageInfo(packageName, flags)
                }
                require(archive != null && archive.packageName == packageName && versionCodeOf(archive) >= versionCodeOf(current)) { "安装包与当前应用或版本不匹配" }
                val currentCerts = signingCertificates(current)
                val archiveCerts = signingCertificates(archive)
                require(currentCerts.isNotEmpty() && archiveCerts == currentCerts) { "安装包签名与已安装版本不一致" }
                cleanupStaleFiles(apk.parentFile)
                runOnUiThread {
                    if (isFinishing || isDestroyed) return@runOnUiThread
                    try {
                        if (packageManager.canRequestPackageInstalls()) {
                            val uri = FileProvider.getUriForFile(this, "$packageName.updates", apk)
                            startActivity(Intent(Intent.ACTION_VIEW).setDataAndType(uri, "application/vnd.android.package-archive").addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION))
                            result.success("已打开系统安装器")
                        } else {
                            startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:$packageName")))
                            result.success("允许此应用安装更新后，返回并再次点击安装")
                        }
                    } catch (e: Exception) { result.error("update", e.localizedMessage, null) }
                }
            } catch (e: Exception) {
                runOnUiThread {
                    if (!isFinishing && !isDestroyed) result.error("update", e.localizedMessage, null)
                }
            }
        }
    }

    @Deprecated("Android activity result")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != 9301) return
        val result = picker ?: return
        picker = null
        result.success(if (resultCode == Activity.RESULT_OK) data?.data?.toString() else null)
    }

    private fun perform(result: MethodChannel.Result, operation: () -> Any?) {
        worker.execute {
            try {
                val value = operation()
                runOnUiThread {
                    if (!isFinishing && !isDestroyed) result.success(value)
                }
            } catch (e: SecurityException) {
                runOnUiThread {
                    if (!isFinishing && !isDestroyed) result.error("document", "文件提供方拒绝访问，请确认文档可写并用系统文件选择器重新打开", null)
                }
            } catch (e: CharacterCodingException) {
                runOnUiThread {
                    if (!isFinishing && !isDestroyed) result.error("document", "所选文件不是有效的 UTF-8 文本", null)
                }
            } catch (e: Exception) {
                runOnUiThread {
                    if (!isFinishing && !isDestroyed) result.error("document", e.localizedMessage ?: "文件操作失败", null)
                }
            }
        }
    }

    private fun read(uri: Uri): String {
        val input = contentResolver.openInputStream(uri) ?: error("无法打开文档")
        val bytes = input.use { stream ->
            val output = java.io.ByteArrayOutputStream()
            val buffer = ByteArray(16384)
            while (true) {
                val n = stream.read(buffer)
                if (n < 0) break
                require(output.size() + n <= 16 * 1024 * 1024) { "配置超过 16 MiB" }
                output.write(buffer, 0, n)
            }
            output.toByteArray()
        }
        return Charsets.UTF_8.newDecoder().decode(java.nio.ByteBuffer.wrap(bytes)).toString()
    }

    private fun write(uri: Uri, text: String) {
        val stream = contentResolver.openOutputStream(uri, "rwt") ?: error("文件提供方拒绝写入")
        stream.use { it.write(text.toByteArray(Charsets.UTF_8)) }
    }

    private fun document(call: MethodCall): Any {
        val uri = Uri.parse(requireNotNull(call.argument<String>("uri")))
        require(uri.scheme == "content") { "需要系统文件选择器返回的 URI" }
        val key = MessageDigest.getInstance("SHA-256").digest(uri.toString().toByteArray()).joinToString("") { "%02x".format(it) }
        val backup = AtomicFile(File(filesDir, "document-$key.bak"))
        if (call.method == "save") {
            val expected = requireNotNull(call.argument<String>("expected"))
            val text = requireNotNull(call.argument<String>("text"))
            val previous = read(uri)
            require(previous == expected) { "文档已在其他应用更改，请重新打开" }
            if (previous != text) {
                val out = backup.startWrite()
                try { out.write(previous.toByteArray(Charsets.UTF_8)); backup.finishWrite(out) }
                catch (e: Exception) { backup.failWrite(out); throw e }
                try {
                    write(uri, text)
                    check(read(uri) == text) { "文件提供方未完整写入配置" }
                } catch (e: Exception) {
                    if (runCatching { read(uri) }.getOrNull() != previous) {
                        runCatching { write(uri, previous) }.getOrElse { error("保存失败，旧内容已留在应用备份中") }
                    }
                    throw e
                }
            }
            return "已保存"
        }
        val label = contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
            if (it.moveToFirst()) it.getString(0) else null
        } ?: "配置文档"
        return mapOf("path" to label, "text" to if (call.method == "backup") backup.openRead().bufferedReader().use { it.readText() } else read(uri), "backup" to backup.baseFile.exists())
    }

    override fun cleanUpFlutterEngine(flutterEngine: FlutterEngine) {
        channel?.setMethodCallHandler(null)
        channel = null
        picker = null
        super.cleanUpFlutterEngine(flutterEngine)
    }

    override fun onDestroy() {
        picker = null
        worker.shutdown()
        super.onDestroy()
    }
}
