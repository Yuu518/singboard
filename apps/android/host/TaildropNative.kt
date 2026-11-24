package io.github.Yuu518.singboard

import android.content.ContentResolver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Process
import android.provider.OpenableColumns
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.Locale
import java.util.UUID

object TaildropNative {
    init {
        System.loadLibrary("singboard_core")
    }

    external fun open(dir: String): Long
    external fun request(handle: Long, requestJson: String): String
    external fun close(handle: Long)
}

data class TaildropServiceCandidate(
    val serviceTag: String,
    val url: String,
)

data class TaildropPeer(
    val stableId: String,
    val displayName: String,
    val os: String,
    val primaryIp: String,
    val online: Boolean,
)

data class TaildropEndpoint(
    val endpointTag: String,
    val networkName: String,
    val peers: List<TaildropPeer>,
)

data class StagedFile(
    val path: String,
    val name: String,
    val size: Long,
)

data class StagedBatch(
    val batchDir: String,
    val files: List<StagedFile>,
)

class TaildropApiException(val code: String, message: String) : Exception(message)

class TaildropClient(dir: String) : AutoCloseable {
    @Volatile
    private var handle: Long = TaildropNative.open(dir)

    private fun ensureHandle(): Long {
        val id = handle
        if (id <= 0L) throw TaildropApiException("closed", "会话已关闭")
        return id
    }

    fun requestRaw(payload: JSONObject): Any? {
        val raw = TaildropNative.request(ensureHandle(), payload.toString())
        val parsed = JSONObject(raw)
        val err = parsed.optJSONObject("error")
        if (err != null) {
            throw TaildropApiException(
                err.optString("code", "error"),
                err.optString("message", "请求失败"),
            )
        }
        return if (parsed.isNull("data")) null else parsed.opt("data")
    }

    fun discover(): List<TaildropServiceCandidate> {
        val list = requestRaw(JSONObject().put("op", "discover")) as? JSONArray ?: return emptyList()
        val out = ArrayList<TaildropServiceCandidate>(list.length())
        for (i in 0 until list.length()) {
            val item = list.optJSONObject(i) ?: continue
            out.add(
                TaildropServiceCandidate(
                    serviceTag = item.optString("serviceTag", ""),
                    url = item.optString("url", ""),
                ),
            )
        }
        return out
    }

    fun connect(serviceTag: String?) {
        val payload = JSONObject().put("op", "connect")
        if (!serviceTag.isNullOrEmpty()) {
            payload.put("serviceTag", serviceTag)
        }
        requestRaw(payload)
    }

    fun disconnect() {
        runCatching { requestRaw(JSONObject().put("op", "disconnect")) }
    }

    fun statusEndpoints(): List<TaildropEndpoint> {
        val snap = requestRaw(JSONObject().put("op", "status")) as? JSONObject ?: return emptyList()
        val rawEndpoints = snap.optJSONArray("endpoints") ?: return emptyList()
        val result = ArrayList<TaildropEndpoint>()
        for (i in 0 until rawEndpoints.length()) {
            val ep = rawEndpoints.optJSONObject(i) ?: continue
            if (!ep.optBoolean("canShareFiles", false)) continue
            val network = ep.optString("networkName", "").lowercase(Locale.ROOT)
            val groups = ArrayList<JSONObject>()
            val rawGroups = ep.optJSONArray("userGroups")
            if (rawGroups != null) {
                for (g in 0 until rawGroups.length()) {
                    rawGroups.optJSONObject(g)?.let { groups.add(it) }
                }
            }
            groups.sortBy { g ->
                val login = g.optString("loginName", "").lowercase(Locale.ROOT)
                if (network.isNotEmpty() && login == network) {
                    0
                } else {
                    val gp = g.optJSONArray("peers")
                    var hasOwn = false
                    if (gp != null) {
                        for (k in 0 until gp.length()) {
                            val item = gp.optJSONObject(k) ?: continue
                            if (!item.optBoolean("shareeNode", false)) {
                                hasOwn = true
                                break
                            }
                        }
                    }
                    if (hasOwn) 1 else 2
                }
            }
            val onlinePeers = ArrayList<TaildropPeer>()
            val offlinePeers = ArrayList<TaildropPeer>()
            for (group in groups) {
                val rawPeers = group.optJSONArray("peers") ?: continue
                for (p in 0 until rawPeers.length()) {
                    val peer = rawPeers.optJSONObject(p) ?: continue
                    if (!peer.optBoolean("canReceiveFiles", false)) continue
                    val stableId = peer.optString("stableId", "").trim()
                    if (stableId.isEmpty()) continue
                    val hostName = peer.optString("hostName", "").trim()
                    val hostOk = hostName.isNotEmpty() && hostName != "device-of-shared-to-user"
                    val dnsName = peer.optString("dnsName", "").trim().trimEnd('.')
                    val dnsLabel = dnsName.substringBefore('.').trim()
                    val dnsOk = dnsLabel.isNotEmpty() && dnsLabel != "device-of-shared-to-user"
                    val displayName = when {
                        dnsOk && hostOk && hostName.equals(dnsLabel, ignoreCase = true) -> hostName
                        dnsOk -> dnsLabel
                        hostOk -> hostName
                        hostName == "device-of-shared-to-user" -> "共享设备"
                        else -> stableId
                    }
                    val ips = peer.optJSONArray("tailscaleIps")
                    val primaryIp = if (ips != null && ips.length() > 0) ips.optString(0, "") else ""
                    val online = peer.optBoolean("online", false)
                    val entry = TaildropPeer(
                        stableId = stableId,
                        displayName = displayName,
                        os = peer.optString("os", ""),
                        primaryIp = primaryIp,
                        online = online,
                    )
                    if (online) onlinePeers.add(entry) else offlinePeers.add(entry)
                }
            }
            val peers = ArrayList<TaildropPeer>(onlinePeers.size + offlinePeers.size)
            peers.addAll(onlinePeers)
            peers.addAll(offlinePeers)
            result.add(
                TaildropEndpoint(
                    endpointTag = ep.optString("endpointTag", ""),
                    networkName = ep.optString("networkName", ""),
                    peers = peers,
                ),
            )
        }
        return result
    }

    fun openUpload(endpointTag: String, peerStableId: String, paths: List<String>): Long {
        val arr = JSONArray()
        for (path in paths) arr.put(path)
        val payload = JSONObject()
            .put("op", "open")
            .put("feed", "upload")
            .put("tag", endpointTag)
            .put("peerStableId", peerStableId)
            .put("paths", arr)
        val value = requestRaw(payload) as? Number
            ?: throw TaildropApiException("invalid", "无法创建上传句柄")
        return value.toLong()
    }

    fun drain(feedHandle: Long): JSONObject {
        val data = requestRaw(
            JSONObject()
                .put("op", "drain")
                .put("handle", feedHandle),
        )
        return data as? JSONObject ?: JSONObject()
    }

    fun closeFeed(feedHandle: Long) {
        runCatching {
            requestRaw(
                JSONObject()
                    .put("op", "close")
                    .put("handle", feedHandle),
            )
        }
    }

    override fun close() {
        val id = handle
        handle = 0L
        if (id > 0L) {
            TaildropNative.close(id)
        }
    }
}

object TaildropStaging {
    fun stageSendIntent(context: Context, intent: Intent): StagedBatch {
        val action = intent.action
        require(action == Intent.ACTION_SEND || action == Intent.ACTION_SEND_MULTIPLE) { "不支持的分享类型" }
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
        val read = Intent.FLAG_GRANT_READ_URI_PERMISSION
        for (uri in uris) {
            val authority = uri.authority?.substringAfterLast('@')
            require(
                uri.scheme == ContentResolver.SCHEME_CONTENT &&
                    !authority.isNullOrEmpty() &&
                    authority != "${context.packageName}.updates" &&
                    uri in grantUris &&
                    (intent.flags and read) != 0 &&
                    context.checkUriPermission(uri, Process.myPid(), Process.myUid(), read) ==
                    PackageManager.PERMISSION_GRANTED,
            ) { "分享内容缺少可用的读取授权" }
        }
        return stageBatch(context, "taildrop-share") { dir ->
            if (uris.isNotEmpty()) {
                uris.map { stageUri(context, it, dir) }
            } else {
                val file = File(dir, "shared-text.txt")
                file.writeText(requireNotNull(text), Charsets.UTF_8)
                listOf(StagedFile(file.absolutePath, file.name, file.length()))
            }
        }
    }

    fun stageBatch(
        context: Context,
        root: String,
        stage: (File) -> List<StagedFile>,
    ): StagedBatch {
        val dir = File(File(context.cacheDir, root), UUID.randomUUID().toString())
        check(dir.mkdirs()) { "无法创建临时目录" }
        try {
            return StagedBatch(dir.absolutePath, stage(dir))
        } catch (e: Exception) {
            dir.deleteRecursively()
            throw e
        }
    }

    fun stageUri(context: Context, uri: Uri, batchDir: File): StagedFile {
        val name = queryDisplayName(context, uri, "file-${System.currentTimeMillis()}")
        val dest = uniqueFile(batchDir, name)
        val input = context.contentResolver.openInputStream(uri) ?: error("无法读取分享文件")
        input.use { src ->
            dest.outputStream().use { dst ->
                src.copyTo(dst, 65536)
            }
        }
        return StagedFile(dest.absolutePath, dest.name, dest.length())
    }

    fun queryDisplayName(context: Context, uri: Uri, fallback: String): String {
        val raw = runCatching {
            context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
                if (it.moveToFirst()) it.getString(0) else null
            }
        }.getOrNull()
        val cleaned = (raw ?: fallback).replace(Regex("[\\\\/:*?\"<>|]"), "_").trim()
        return if (cleaned.isEmpty() || cleaned == "." || cleaned == "..") fallback else cleaned
    }

    fun uniqueFile(dir: File, name: String): File {
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

    fun clearBatchDir(context: Context, dirPath: String?) {
        if (dirPath.isNullOrEmpty()) return
        runCatching {
            val target = File(dirPath).canonicalFile
            val roots = listOf("taildrop-share", "taildrop-pick").map {
                File(context.cacheDir, it).canonicalFile
            }
            val batchId = runCatching { UUID.fromString(target.name) }.getOrNull()
            if (roots.any { it == target.parentFile } && batchId?.toString() == target.name) {
                target.deleteRecursively()
            }
        }
    }

    fun formatBytes(bytes: Long): String {
        val b = bytes.coerceAtLeast(0L).toDouble()
        return when {
            b >= 1024.0 * 1024 * 1024 -> String.format(Locale.US, "%.2f GB", b / (1024.0 * 1024 * 1024))
            b >= 1024.0 * 1024 -> String.format(Locale.US, "%.1f MB", b / (1024.0 * 1024))
            b >= 1024.0 -> String.format(Locale.US, "%.1f KB", b / 1024.0)
            else -> "${bytes.coerceAtLeast(0L)} B"
        }
    }
}
