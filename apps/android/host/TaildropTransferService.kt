package io.github.Yuu518.singboard

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.SystemClock
import android.widget.Toast
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

class TaildropTransferService : Service() {
    companion object {
        private const val ACTION_START = "io.github.Yuu518.singboard.action.TAILDROP_START"
        private const val ACTION_CANCEL = "io.github.Yuu518.singboard.action.TAILDROP_CANCEL"
        private const val EXTRA_BATCH_DIR = "batchDir"
        private const val EXTRA_PATHS = "paths"
        private const val EXTRA_TOTAL_BYTES = "totalBytes"
        private const val EXTRA_SERVICE_TAG = "serviceTag"
        private const val EXTRA_ENDPOINT_TAG = "endpointTag"
        private const val EXTRA_PEER_ID = "peerId"
        private const val EXTRA_PEER_NAME = "peerName"
        private const val EXTRA_JOB_ID = "jobId"
        private const val CHANNEL_PROGRESS = "taildrop_transfer"
        private const val CHANNEL_RESULT = "taildrop_result"
        private const val ONGOING_NOTIFICATION_ID = 41001
        private val nextJobId = AtomicInteger(1)
        private val nextResultNotifId = AtomicInteger(41100)

        fun enqueue(
            context: Context,
            batchDir: String,
            paths: List<String>,
            totalBytes: Long,
            serviceTag: String?,
            endpointTag: String,
            peerId: String,
            peerName: String,
        ) {
            val intent = Intent(context, TaildropTransferService::class.java).apply {
                action = ACTION_START
                putExtra(EXTRA_BATCH_DIR, batchDir)
                putStringArrayListExtra(EXTRA_PATHS, ArrayList(paths))
                putExtra(EXTRA_TOTAL_BYTES, totalBytes)
                putExtra(EXTRA_SERVICE_TAG, serviceTag ?: "")
                putExtra(EXTRA_ENDPOINT_TAG, endpointTag)
                putExtra(EXTRA_PEER_ID, peerId)
                putExtra(EXTRA_PEER_NAME, peerName)
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
        }
    }

    private data class TransferJob(
        val jobId: Int,
        val batchDir: String,
        val paths: List<String>,
        val totalBytes: Long,
        val serviceTag: String,
        val endpointTag: String,
        val peerId: String,
        val peerName: String,
        val cancelled: AtomicBoolean = AtomicBoolean(false),
    )

    private val worker = Executors.newSingleThreadExecutor()
    private val mainHandler = Handler(Looper.getMainLooper())
    private val pendingCount = AtomicInteger(0)
    @Volatile
    private var activeJob: TransferJob? = null
    @Volatile
    private var activeClient: TaildropClient? = null
    @Volatile
    private var activeFeed: Long = 0L

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        ensureChannels()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_CANCEL -> {
                val targetId = intent.getIntExtra(EXTRA_JOB_ID, -1)
                val job = activeJob
                if (job != null && (targetId < 0 || job.jobId == targetId)) {
                    job.cancelled.set(true)
                    val feed = activeFeed
                    val client = activeClient
                    if (client != null && feed > 0L) {
                        Thread {
                            client.closeFeed(feed)
                            client.close()
                        }.start()
                    }
                }
                return START_NOT_STICKY
            }
            ACTION_START -> {
                val batchDir = intent.getStringExtra(EXTRA_BATCH_DIR).orEmpty()
                val paths = intent.getStringArrayListExtra(EXTRA_PATHS) ?: arrayListOf()
                val totalBytes = intent.getLongExtra(EXTRA_TOTAL_BYTES, 0L)
                val serviceTag = intent.getStringExtra(EXTRA_SERVICE_TAG).orEmpty()
                val endpointTag = intent.getStringExtra(EXTRA_ENDPOINT_TAG).orEmpty()
                val peerId = intent.getStringExtra(EXTRA_PEER_ID).orEmpty()
                val peerName = intent.getStringExtra(EXTRA_PEER_NAME).orEmpty().ifEmpty { "设备" }
                if (batchDir.isEmpty() || paths.isEmpty() || endpointTag.isEmpty() || peerId.isEmpty()) {
                    TaildropStaging.clearBatchDir(this, batchDir)
                    if (pendingCount.get() == 0) stopSelf()
                    return START_NOT_STICKY
                }
                val job = TransferJob(
                    jobId = nextJobId.getAndIncrement(),
                    batchDir = batchDir,
                    paths = paths,
                    totalBytes = totalBytes,
                    serviceTag = serviceTag,
                    endpointTag = endpointTag,
                    peerId = peerId,
                    peerName = peerName,
                )
                promoteForeground(buildProgressNotification(job, 0, 0L))
                pendingCount.incrementAndGet()
                worker.execute { runJob(job) }
                return START_NOT_STICKY
            }
            else -> {
                if (pendingCount.get() == 0) stopSelf()
                return START_NOT_STICKY
            }
        }
    }

    override fun onDestroy() {
        activeJob?.cancelled?.set(true)
        worker.shutdownNow()
        super.onDestroy()
    }

    private fun promoteForeground(notification: Notification) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(
                ONGOING_NOTIFICATION_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC,
            )
        } else {
            startForeground(ONGOING_NOTIFICATION_ID, notification)
        }
    }

    private fun runJob(job: TransferJob) {
        activeJob = job
        var client: TaildropClient? = null
        var feedHandle = 0L
        var errorMessage: String? = null
        var succeeded = false
        try {
            updateProgressNotification(job, 0, 0L)
            client = TaildropClient(filesDir.absolutePath)
            activeClient = client
            client.connect(job.serviceTag.ifEmpty { null })
            if (job.cancelled.get()) return
            feedHandle = client.openUpload(job.endpointTag, job.peerId, job.paths)
            activeFeed = feedHandle
            var completedFiles = 0
            var sentBytes = 0L
            var lastNotifyAt = 0L
            while (!job.cancelled.get()) {
                val batch = client.drain(feedHandle)
                val events = batch.optJSONArray("events")
                if (events != null) {
                    for (i in 0 until events.length()) {
                        val ev = events.optJSONObject(i) ?: continue
                        val errObj = ev.optJSONObject("error")
                        if (errObj != null) {
                            errorMessage = errObj.optString("message", "发送失败")
                            break
                        }
                        if (ev.has("totalSentBytes")) {
                            sentBytes = ev.optLong("totalSentBytes", sentBytes)
                        }
                        if (ev.has("completedFiles")) {
                            completedFiles = ev.optInt("completedFiles", completedFiles)
                        }
                    }
                }
                if (errorMessage != null) break
                val now = SystemClock.elapsedRealtime()
                if (now - lastNotifyAt >= 250L) {
                    updateProgressNotification(job, completedFiles, sentBytes)
                    lastNotifyAt = now
                }
                if (batch.optBoolean("finished", false)) {
                    succeeded = true
                    break
                }
                Thread.sleep(160L)
            }
        } catch (e: InterruptedException) {
            Thread.currentThread().interrupt()
        } catch (e: Exception) {
            if (!job.cancelled.get()) {
                errorMessage = e.localizedMessage ?: "发送失败"
            }
        } finally {
            activeFeed = 0L
            activeClient = null
            if (client != null) {
                if (feedHandle > 0L) client.closeFeed(feedHandle)
                client.close()
            }
            TaildropStaging.clearBatchDir(this, job.batchDir)
            activeJob = null
            when {
                job.cancelled.get() -> {
                    showToast("已取消发送到 ${job.peerName}")
                }
                succeeded -> {
                    val msg = "已发送 ${job.paths.size} 个文件到 ${job.peerName}"
                    postResultNotification("Taildrop 发送完成", msg)
                    showToast(msg)
                }
                else -> {
                    val msg = errorMessage ?: "发送失败"
                    postResultNotification("Taildrop 发送失败", msg)
                    showToast("发送失败：$msg")
                }
            }
            if (pendingCount.decrementAndGet() <= 0) {
                mainHandler.post {
                    if (pendingCount.get() <= 0) {
                        stopForeground(STOP_FOREGROUND_REMOVE)
                        stopSelf()
                    }
                }
            }
        }
    }

    private fun ensureChannels() {
        val manager = getSystemService(NotificationManager::class.java) ?: return
        if (manager.getNotificationChannel(CHANNEL_PROGRESS) == null) {
            manager.createNotificationChannel(
                NotificationChannel(
                    CHANNEL_PROGRESS,
                    "Taildrop 文件传输",
                    NotificationManager.IMPORTANCE_LOW,
                ).apply {
                    description = "通过 Taildrop 后台发送文件时的实时进度"
                    setShowBadge(false)
                },
            )
        }
        if (manager.getNotificationChannel(CHANNEL_RESULT) == null) {
            manager.createNotificationChannel(
                NotificationChannel(
                    CHANNEL_RESULT,
                    "Taildrop 传输结果",
                    NotificationManager.IMPORTANCE_DEFAULT,
                ).apply {
                    description = "Taildrop 文件发送完成或失败通知"
                },
            )
        }
    }

    private fun buildProgressNotification(
        job: TransferJob,
        completedFiles: Int,
        sentBytes: Long,
    ): Notification {
        val cancelIntent = Intent(this, TaildropTransferService::class.java).apply {
            action = ACTION_CANCEL
            putExtra(EXTRA_JOB_ID, job.jobId)
        }
        val cancelPending = PendingIntent.getService(
            this,
            job.jobId,
            cancelIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val totalBytes = job.totalBytes
        val progressText = if (totalBytes > 0L) {
            "$completedFiles/${job.paths.size} · ${TaildropStaging.formatBytes(sentBytes)} / ${TaildropStaging.formatBytes(totalBytes)}"
        } else {
            "$completedFiles/${job.paths.size}"
        }
        val builder = Notification.Builder(this, CHANNEL_PROGRESS)
            .setSmallIcon(applicationInfo.icon)
            .setContentTitle("正在发送到 ${job.peerName}")
            .setContentText(progressText)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .addAction(Notification.Action.Builder(null, "取消", cancelPending).build())
        if (totalBytes > 0L) {
            val pct = ((sentBytes.coerceAtMost(totalBytes) * 1000L) / totalBytes).toInt()
            builder.setProgress(1000, pct, false)
        } else {
            builder.setProgress(0, 0, true)
        }
        return builder.build()
    }

    private fun updateProgressNotification(
        job: TransferJob,
        completedFiles: Int,
        sentBytes: Long,
    ) {
        val manager = getSystemService(NotificationManager::class.java) ?: return
        manager.notify(ONGOING_NOTIFICATION_ID, buildProgressNotification(job, completedFiles, sentBytes))
    }

    private fun postResultNotification(title: String, text: String) {
        val manager = getSystemService(NotificationManager::class.java) ?: return
        val notif = Notification.Builder(this, CHANNEL_RESULT)
            .setSmallIcon(applicationInfo.icon)
            .setContentTitle(title)
            .setContentText(text)
            .setAutoCancel(true)
            .build()
        manager.notify(nextResultNotifId.getAndIncrement(), notif)
    }

    private fun showToast(message: String) {
        mainHandler.post {
            Toast.makeText(applicationContext, message, Toast.LENGTH_SHORT).show()
        }
    }
}
