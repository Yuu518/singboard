package io.github.Yuu518.singboard

import android.app.Activity
import android.content.ComponentName
import android.content.ContentResolver
import android.content.Intent
import android.content.pm.PackageInfo
import android.content.pm.PackageManager
import android.graphics.Color
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Process
import android.provider.OpenableColumns
import android.provider.Settings
import androidx.core.content.FileProvider
import androidx.core.view.WindowCompat
import android.util.AtomicFile
import android.view.DragEvent
import android.widget.Toast
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import java.io.File
import java.nio.charset.CharacterCodingException
import java.security.MessageDigest
import java.util.ArrayDeque
import java.util.UUID
import java.util.concurrent.Executors

class MainActivity : FlutterActivity() {
    private val worker = Executors.newSingleThreadExecutor()
    private var picker: MethodChannel.Result? = null
    private var multiPicker: MethodChannel.Result? = null
    private var pemPicker: MethodChannel.Result? = null
    private var savePicker: Pair<MethodChannel.Result, File>? = null
    private val pendingShares = ArrayDeque<Map<String, Any>>()
    private var stagingShares = 0
    private var dartReady = false
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
        handleShareIntent(intent)
        setupDragAndDrop()
    }

    private fun setupDragAndDrop() {
        window.decorView.setOnDragListener { _, event ->
            when (event.action) {
                DragEvent.ACTION_DRAG_STARTED -> true
                DragEvent.ACTION_DROP -> {
                    val clip = event.clipData ?: return@setOnDragListener false
                    val uris = linkedSetOf<Uri>()
                    val texts = mutableListOf<String>()
                    for (i in 0 until clip.itemCount) {
                        val item = clip.getItemAt(i)
                        if (item.uri != null) {
                            uris.add(item.uri)
                        } else if (!item.text.isNullOrEmpty()) {
                            texts.add(item.text.toString())
                        }
                    }
                    if (uris.isEmpty() && texts.isEmpty()) return@setOnDragListener false
                    val perm = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
                        requestDragAndDropPermissions(event)
                    } else {
                        null
                    }
                    stagingShares++
                    worker.execute {
                        try {
                            val payload = stageBatch("taildrop-share") { dir ->
                                if (uris.isNotEmpty()) {
                                    uris.map { stageUri(it, dir) }
                                } else {
                                    val file = File(dir, "dropped-text.txt")
                                    file.writeText(texts.joinToString("\n"), Charsets.UTF_8)
                                    listOf(
                                        mapOf(
                                            "path" to file.absolutePath,
                                            "name" to file.name,
                                            "size" to file.length(),
                                        ),
                                    )
                                }
                            } + ("fromExternal" to false)
                            runOnUiThread {
                                stagingShares--
                                if (isFinishing || isDestroyed) {
                                    File(payload["batchDir"] as String).deleteRecursively()
                                } else {
                                    pendingShares.addLast(payload)
                                    if (dartReady) channel?.invokeMethod("taildropShareIncoming", null)
                                }
                            }
                        } catch (e: Exception) {
                            runOnUiThread {
                                stagingShares--
                                if (!isFinishing && !isDestroyed) {
                                    Toast.makeText(
                                        this,
                                        "无法导入拖入的文件：${e.localizedMessage}",
                                        Toast.LENGTH_LONG,
                                    ).show()
                                }
                            }
                        } finally {
                            perm?.release()
                        }
                    }
                    true
                }
                else -> true
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleShareIntent(intent)
    }

    private fun handleShareIntent(intent: Intent?) {
        val action = intent?.action ?: return
        if (action != Intent.ACTION_SEND && action != Intent.ACTION_SEND_MULTIPLE) return
        stagingShares++
        worker.execute {
            try {
                val uris = linkedSetOf<Uri>()
                val grantUris = linkedSetOf<Uri>()
                intent.data?.let { grantUris.add(it) }
                intent.clipData?.let { clip ->
                    for (i in 0 until clip.itemCount) {
                        clip.getItemAt(i).uri?.let { grantUris.add(it) }
                    }
                }
                @Suppress("DEPRECATION")
                if (action == Intent.ACTION_SEND) {
                    intent.getParcelableExtra<Uri>(Intent.EXTRA_STREAM)?.let { uris.add(it) }
                } else {
                    intent.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM)?.let { uris.addAll(it) }
                }
                uris.addAll(grantUris)
                val text = intent.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString()
                require(uris.isNotEmpty() || !text.isNullOrEmpty()) { "分享内容为空" }
                for (uri in uris) {
                    val authority = uri.authority?.substringAfterLast('@')
                    val read = Intent.FLAG_GRANT_READ_URI_PERMISSION
                    require(
                        uri.scheme == ContentResolver.SCHEME_CONTENT && !authority.isNullOrEmpty() &&
                            authority != "$packageName.updates" && uri in grantUris &&
                            (intent.flags and read) != 0 &&
                            checkUriPermission(uri, Process.myPid(), Process.myUid(), read) == PackageManager.PERMISSION_GRANTED
                    ) { "分享内容缺少可用的读取授权" }
                }
                val payload = stageBatch("taildrop-share") { dir ->
                    if (uris.isNotEmpty()) {
                        uris.map { stageUri(it, dir) }
                    } else {
                        val file = File(dir, "shared-text.txt")
                        file.writeText(requireNotNull(text), Charsets.UTF_8)
                        listOf(mapOf("path" to file.absolutePath, "name" to file.name, "size" to file.length()))
                    }
                } + ("fromExternal" to true)
                runOnUiThread {
                    stagingShares--
                    if (isFinishing || isDestroyed) {
                        File(payload["batchDir"] as String).deleteRecursively()
                    } else {
                        pendingShares.addLast(payload)
                        if (dartReady) channel?.invokeMethod("taildropShareIncoming", null)
                    }
                }
            } catch (e: Exception) {
                runOnUiThread {
                    stagingShares--
                    if (!isFinishing && !isDestroyed) {
                        Toast.makeText(this, "无法导入分享内容：${e.localizedMessage}", Toast.LENGTH_LONG).show()
                    }
                }
            }
        }
    }

    private fun stageBatch(root: String, stage: (File) -> List<Map<String, Any>>): Map<String, Any> {
        val dir = File(File(cacheDir, root), UUID.randomUUID().toString())
        check(dir.mkdirs()) { "无法创建临时目录" }
        try {
            return mapOf("batchDir" to dir.absolutePath, "files" to stage(dir))
        } catch (e: Exception) {
            dir.deleteRecursively()
            throw e
        }
    }

    private fun queryDisplayName(uri: Uri, fallback: String): String {
        val raw = runCatching {
            contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
                if (it.moveToFirst()) it.getString(0) else null
            }
        }.getOrNull()
        val cleaned = (raw ?: fallback).replace(Regex("[\\\\/:*?\"<>|]"), "_").trim()
        return if (cleaned.isEmpty() || cleaned == "." || cleaned == "..") fallback else cleaned
    }

    private fun uniqueFile(dir: File, name: String): File {
        var candidate = File(dir, name)
        if (!candidate.exists()) return candidate
        val dot = name.lastIndexOf('.')
        val stem = if (dot > 0) name.substring(0, dot) else name
        val ext = if (dot > 0) name.substring(dot) else ""
        var i = 1
        while (candidate.exists()) {
            candidate = File(dir, "$stem ($i)$ext")
            i++
        }
        return candidate
    }

    private fun stageUri(uri: Uri, batchDir: File): Map<String, Any> {
        val name = queryDisplayName(uri, "file-${System.currentTimeMillis()}")
        val dest = uniqueFile(batchDir, name)
        val input = contentResolver.openInputStream(uri) ?: error("无法读取分享文件")
        input.use { src ->
            dest.outputStream().use { dst ->
                src.copyTo(dst, 65536)
            }
        }
        return mapOf(
            "path" to dest.absolutePath,
            "name" to dest.name,
            "size" to dest.length(),
        )
    }

    private fun cleanupStaleFiles(keepUpdateDir: File?) {
        runCatching {
            val updates = File(filesDir, "updates")
            updates.listFiles()?.forEach { child ->
                if (keepUpdateDir != null && child.canonicalFile == keepUpdateDir.canonicalFile) return@forEach
                child.deleteRecursively()
            }
            val cutoff = System.currentTimeMillis() - 7L * 24 * 60 * 60 * 1000
            for (sub in listOf("taildrop-share", "taildrop-pick", "taildrop-download")) {
                val root = File(cacheDir, sub)
                root.listFiles()?.forEach { child ->
                    if (child.lastModified() < System.currentTimeMillis() - 6L * 60 * 60 * 1000) {
                        child.deleteRecursively()
                    }
                }
            }
            val backups = filesDir.listFiles { file ->
                file.isFile && file.name.startsWith("document-") && file.name.endsWith(".bak")
            }?.sortedByDescending { it.lastModified() } ?: emptyList()
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
                "consumePendingShare" -> {
                    dartReady = true
                    val data = pendingShares.pollFirst()
                    if (data == null && stagingShares == 0 && call.argument<Boolean>("finishIfEmpty") == true) {
                        moveTaskToBack(true)
                    }
                    result.success(data)
                }
                "clearStagedShare" -> perform(result) {
                    val dirPath = call.argument<String>("batchDir")
                    if (!dirPath.isNullOrEmpty()) {
                        val target = File(dirPath).canonicalFile
                        val roots = listOf("taildrop-share", "taildrop-pick").map { File(cacheDir, it).canonicalFile }
                        val batchId = runCatching { UUID.fromString(target.name) }.getOrNull()
                        if (roots.any { it == target.parentFile } && batchId?.toString() == target.name) {
                            target.deleteRecursively()
                        }
                    }
                    true
                }
                "setTaildropShareEnabled" -> {
                    val enabled = call.argument<Boolean>("enabled") == true
                    try {
                        val component = ComponentName(packageName, "$packageName.TaildropShareActivity")
                        val state = if (enabled) {
                            PackageManager.COMPONENT_ENABLED_STATE_ENABLED
                        } else {
                            PackageManager.COMPONENT_ENABLED_STATE_DISABLED
                        }
                        packageManager.setComponentEnabledSetting(
                            component,
                            state,
                            PackageManager.DONT_KILL_APP,
                        )
                        result.success(true)
                    } catch (e: Exception) {
                        result.error("share", e.localizedMessage, null)
                    }
                }
                "openUrl" -> {
                    val url = call.argument<String>("url")
                    if (url.isNullOrEmpty()) {
                        result.error("url", "无效链接", null)
                    } else {
                        try {
                            startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)))
                            result.success(true)
                        } catch (e: Exception) {
                            result.error("url", e.localizedMessage, null)
                        }
                    }
                }
                "pickFiles" -> {
                    if (multiPicker != null) {
                        result.error("busy", "文件选择器已打开", null)
                    } else {
                        multiPicker = result
                        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT)
                            .addCategory(Intent.CATEGORY_OPENABLE)
                            .putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
                        intent.type = "*/*"
                        try { startActivityForResult(intent, 9302) }
                        catch (e: Exception) { multiPicker = null; result.error("open", e.localizedMessage, null) }
                    }
                }
                "pickPemFile" -> {
                    if (pemPicker != null) {
                        result.error("busy", "文件选择器已打开", null)
                    } else {
                        pemPicker = result
                        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE)
                        intent.type = "*/*"
                        try { startActivityForResult(intent, 9303) }
                        catch (e: Exception) { pemPicker = null; result.error("open", e.localizedMessage, null) }
                    }
                }
                "taildropDownloadTemp" -> perform(result) {
                    val name = (call.argument<String>("name") ?: "download").replace(Regex("[\\\\/:*?\"<>|]"), "_")
                    val dir = File(File(cacheDir, "taildrop-download"), UUID.randomUUID().toString())
                    dir.mkdirs()
                    File(dir, name).absolutePath
                }
                "saveTaildropDownload" -> {
                    val srcPath = call.argument<String>("path")
                    val name = call.argument<String>("name") ?: "download"
                    if (savePicker != null || srcPath.isNullOrEmpty()) {
                        result.error("busy", "保存选择器不可用", null)
                    } else {
                        savePicker = Pair(result, File(srcPath))
                        val intent = Intent(Intent.ACTION_CREATE_DOCUMENT)
                            .addCategory(Intent.CATEGORY_OPENABLE)
                            .putExtra(Intent.EXTRA_TITLE, name)
                        intent.type = "*/*"
                        try { startActivityForResult(intent, 9304) }
                        catch (e: Exception) { savePicker = null; result.error("save", e.localizedMessage, null) }
                    }
                }
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
        if (requestCode == 9301) {
            val result = picker ?: return
            picker = null
            result.success(if (resultCode == Activity.RESULT_OK) data?.data?.toString() else null)
            return
        }
        if (requestCode == 9302) {
            val result = multiPicker ?: return
            multiPicker = null
            if (resultCode != Activity.RESULT_OK || data == null) {
                result.success(null)
                return
            }
            val uris = mutableListOf<Uri>()
            val clip = data.clipData
            if (clip != null) {
                for (i in 0 until clip.itemCount) {
                    clip.getItemAt(i).uri?.let { uris.add(it) }
                }
            } else {
                data.data?.let { uris.add(it) }
            }
            if (uris.isEmpty()) {
                result.success(null)
                return
            }
            perform(result) {
                stageBatch("taildrop-pick") { dir -> uris.map { stageUri(it, dir) } }
            }
            return
        }
        if (requestCode == 9303) {
            val result = pemPicker ?: return
            pemPicker = null
            val uri = if (resultCode == Activity.RESULT_OK) data?.data else null
            if (uri == null) {
                result.success(null)
                return
            }
            perform(result) { read(uri) }
            return
        }
        if (requestCode == 9304) {
            val pending = savePicker ?: return
            savePicker = null
            val (result, srcFile) = pending
            val destUri = if (resultCode == Activity.RESULT_OK) data?.data else null
            if (destUri == null) {
                worker.execute { runCatching { srcFile.parentFile?.deleteRecursively() } }
                result.success(false)
                return
            }
            perform(result) {
                val out = contentResolver.openOutputStream(destUri, "w") ?: error("无法写入目标文件")
                srcFile.inputStream().use { src ->
                    out.use { dst -> src.copyTo(dst, 65536) }
                }
                runCatching { srcFile.parentFile?.deleteRecursively() }
                true
            }
        }
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
        dartReady = false
        picker = null
        multiPicker = null
        pemPicker = null
        savePicker = null
        super.cleanUpFlutterEngine(flutterEngine)
    }

    override fun onDestroy() {
        picker = null
        multiPicker = null
        pemPicker = null
        savePicker = null
        val abandoned = pendingShares.map { File(it["batchDir"] as String) }
        pendingShares.clear()
        worker.execute { abandoned.forEach { it.deleteRecursively() } }
        worker.shutdown()
        super.onDestroy()
    }
}
