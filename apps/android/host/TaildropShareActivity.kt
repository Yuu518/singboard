package io.github.Yuu518.singboard

import android.app.Activity
import android.content.Intent
import android.content.res.Configuration
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.os.Bundle
import android.text.TextUtils
import android.transition.AutoTransition
import android.transition.TransitionManager
import android.util.TypedValue
import android.view.Gravity
import android.view.MotionEvent
import android.view.VelocityTracker
import android.view.View
import android.view.ViewConfiguration
import android.view.ViewGroup
import android.view.ViewTreeObserver
import android.view.animation.PathInterpolator
import android.widget.FrameLayout
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicInteger

class TaildropShareActivity : Activity() {
    private val worker = Executors.newSingleThreadExecutor()
    private val connectEpoch = AtomicInteger(0)

    private var batchDir: String = ""
    private val files = mutableListOf<StagedFile>()
    private var staging = true
    private var connecting = true
    private var stageError = ""
    private var syncError = ""
    private var services: List<TaildropServiceCandidate> = emptyList()
    private var selectedServiceTag: String? = null
    private var endpoints: List<TaildropEndpoint> = emptyList()
    private var selectedEndpointTag: String? = null
    private var selectedPeerId: String? = null
    private var handedOff = false
    private var closing = false
    private var client: TaildropClient? = null

    private lateinit var rootOverlay: FrameLayout
    private lateinit var scrimView: View
    private lateinit var sheetCard: LinearLayout
    private lateinit var sheetScrollView: ScrollView
    private var lastVisualMode = -1
    private lateinit var subtitleView: TextView
    private lateinit var filesHeaderCount: TextView
    private lateinit var filesHeaderSize: TextView
    private lateinit var filesListBox: LinearLayout
    private lateinit var servicesRow: HorizontalScrollView
    private lateinit var servicesChips: LinearLayout
    private lateinit var endpointsRow: HorizontalScrollView
    private lateinit var endpointsChips: LinearLayout
    private lateinit var statusBox: LinearLayout
    private lateinit var statusSpinner: ProgressBar
    private lateinit var statusLabel: TextView
    private lateinit var peersHeader: TextView
    private lateinit var peersListBox: LinearLayout
    private lateinit var sendButton: TextView

    private var darkMode = false

    private fun dp(value: Float): Int =
        TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, value, resources.displayMetrics).toInt()

    private fun inkColor(): Int = if (darkMode) 0xFFF2F2F7.toInt() else 0xFF1C1C1E.toInt()
    private fun mutedColor(): Int = if (darkMode) 0xFFB5B5C0.toInt() else 0xFF5C5C66.toInt()
    private fun cardBgColor(): Int = if (darkMode) 0xF21E2028.toInt() else 0xF7FAF9FC.toInt()
    private fun cardBorderColor(): Int = if (darkMode) 0x33FFFFFF.toInt() else 0x1A000000.toInt()
    private fun fillColor(): Int = if (darkMode) 0x2E767680.toInt() else 0x1A767680.toInt()
    private fun subtleBorderColor(): Int = if (darkMode) 0x26FFFFFF.toInt() else 0x1A000000.toInt()
    private fun selectedBgColor(): Int = if (darkMode) 0x38DCE4FF.toInt() else 0x241E2438.toInt()
    private fun selectedBorderColor(): Int = if (darkMode) 0x66DCE4FF.toInt() else 0x4D1E2438.toInt()
    private fun selectedInkColor(): Int = if (darkMode) 0xFFF6F8FF.toInt() else 0xFF141824.toInt()
    private fun successColor(): Int = if (darkMode) 0xFF30D158.toInt() else 0xFF34C759.toInt()
    private fun dangerColor(): Int = if (darkMode) 0xFFFF8F87.toInt() else 0xFFB5150B.toInt()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            overrideActivityTransition(OVERRIDE_TRANSITION_OPEN, 0, 0)
            overrideActivityTransition(OVERRIDE_TRANSITION_CLOSE, 0, 0)
        } else {
            @Suppress("DEPRECATION")
            overridePendingTransition(0, 0)
        }
        darkMode = (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) ==
            Configuration.UI_MODE_NIGHT_YES

        WindowCompat.setDecorFitsSystemWindows(window, false)
        @Suppress("DEPRECATION")
        run {
            window.statusBarColor = Color.TRANSPARENT
            window.navigationBarColor = Color.TRANSPARENT
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            window.isNavigationBarContrastEnforced = false
        }
        val controller = WindowCompat.getInsetsController(window, window.decorView)
        controller.isAppearanceLightStatusBars = !darkMode
        controller.isAppearanceLightNavigationBars = !darkMode

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            onBackInvokedDispatcher.registerOnBackInvokedCallback(
                android.window.OnBackInvokedDispatcher.PRIORITY_DEFAULT,
            ) {
                closeNow()
            }
        }

        buildLayout()
        animateEnter()
        startShareFlow(intent)
    }

    @Suppress("DEPRECATION")
    override fun onBackPressed() {
        closeNow()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        if (!handedOff && batchDir.isNotEmpty()) {
            val prev = batchDir
            batchDir = ""
            worker.execute { TaildropStaging.clearBatchDir(applicationContext, prev) }
        }
        handedOff = false
        startShareFlow(intent)
    }

    override fun onDestroy() {
        connectEpoch.incrementAndGet()
        val oldClient = client
        client = null
        val staleDir = if (!handedOff) batchDir else ""
        batchDir = ""
        worker.execute {
            oldClient?.close()
            if (staleDir.isNotEmpty()) {
                TaildropStaging.clearBatchDir(applicationContext, staleDir)
            }
        }
        worker.shutdown()
        super.onDestroy()
    }

    private fun closeNow() {
        if (closing || isFinishing) return
        closing = true
        scrimView.isClickable = false
        sheetCard.isClickable = false
        scrimView.animate().cancel()
        sheetCard.animate().cancel()
        var done = false
        val finishTask = Runnable {
            if (done) return@Runnable
            done = true
            if (!isFinishing && !isDestroyed) {
                finishAndRemoveTask()
                if (Build.VERSION.SDK_INT < Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                    @Suppress("DEPRECATION")
                    overridePendingTransition(0, 0)
                }
            }
        }
        val travel = (sheetCard.height.takeIf { it > 0 } ?: dp(320f)) + dp(28f)
        val remaining = ((travel - sheetCard.translationY) / travel.coerceAtLeast(1)).coerceIn(0.25f, 1f)
        val duration = (200f * remaining).toLong().coerceIn(70L, 200L)
        val exitCurve = PathInterpolator(0.32f, 0f, 0.67f, 0f)
        rootOverlay.postDelayed(finishTask, duration + 40L)
        scrimView.animate()
            .alpha(0f)
            .setDuration(duration)
            .setInterpolator(exitCurve)
            .start()
        sheetCard.animate()
            .translationY(travel.toFloat())
            .setDuration(duration)
            .setInterpolator(exitCurve)
            .withEndAction(finishTask)
            .start()
    }

    private fun animateEnter() {
        scrimView.alpha = 0f
        sheetCard.alpha = 0f
        sheetCard.viewTreeObserver.addOnPreDrawListener(
            object : ViewTreeObserver.OnPreDrawListener {
                override fun onPreDraw(): Boolean {
                    sheetCard.viewTreeObserver.removeOnPreDrawListener(this)
                    if (closing || isFinishing) return true
                    val travel = (sheetCard.height.takeIf { it > 0 } ?: dp(260f)) + dp(28f)
                    sheetCard.translationY = travel.toFloat()
                    sheetCard.alpha = 1f
                    val enterCurve = PathInterpolator(0.32f, 0.72f, 0f, 1f)
                    scrimView.animate()
                        .alpha(1f)
                        .setDuration(240L)
                        .setInterpolator(enterCurve)
                        .start()
                    sheetCard.animate()
                        .translationY(0f)
                        .setDuration(280L)
                        .setInterpolator(enterCurve)
                        .start()
                    return true
                }
            },
        )
    }

    private fun roundedBg(fill: Int, radiusDp: Float, strokeColor: Int = 0, strokeDp: Float = 0f): GradientDrawable =
        GradientDrawable().apply {
            setColor(fill)
            cornerRadius = dp(radiusDp).toFloat()
            if (strokeColor != 0 && strokeDp > 0f) {
                setStroke(dp(strokeDp).coerceAtLeast(1), strokeColor)
            }
        }

    private fun buildLayout() {
        rootOverlay = FrameLayout(this)
        scrimView = View(this).apply {
            setBackgroundColor(if (darkMode) 0x6B000000 else 0x45000000)
            setOnClickListener { closeNow() }
        }
        rootOverlay.addView(
            scrimView,
            FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT,
            ),
        )

        val maxSheetWidth = dp(520f)
        val screenWidth = resources.displayMetrics.widthPixels
        val cardWidth = (screenWidth - dp(20f)).coerceAtMost(maxSheetWidth)

        val touchSlop = ViewConfiguration.get(this).scaledTouchSlop
        val dismissVelocity = dp(650f).toFloat()
        sheetCard = object : LinearLayout(this) {
            private var downX = 0f
            private var downY = 0f
            private var dragStartY = 0f
            private var dragging = false
            private var velocityTracker: VelocityTracker? = null

            private fun canSheetScrollUp(event: MotionEvent): Boolean {
                if (!::sheetScrollView.isInitialized) return false
                val loc = IntArray(2)
                sheetScrollView.getLocationOnScreen(loc)
                val inScroll = event.rawY >= loc[1] && event.rawY <= loc[1] + sheetScrollView.height
                return inScroll && sheetScrollView.canScrollVertically(-1)
            }

            override fun onInterceptTouchEvent(ev: MotionEvent): Boolean {
                if (closing) return true
                when (ev.actionMasked) {
                    MotionEvent.ACTION_DOWN -> {
                        downX = ev.rawX
                        downY = ev.rawY
                        dragStartY = ev.rawY
                        dragging = false
                        velocityTracker?.recycle()
                        velocityTracker = VelocityTracker.obtain().apply { addMovement(ev) }
                    }
                    MotionEvent.ACTION_MOVE -> {
                        velocityTracker?.addMovement(ev)
                        val dy = ev.rawY - downY
                        val dx = ev.rawX - downX
                        if (dy > touchSlop && dy > kotlin.math.abs(dx) && !canSheetScrollUp(ev)) {
                            dragging = true
                            dragStartY = ev.rawY
                            animate().cancel()
                            scrimView.animate().cancel()
                            return true
                        }
                    }
                    MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                        dragging = false
                        velocityTracker?.recycle()
                        velocityTracker = null
                    }
                }
                return super.onInterceptTouchEvent(ev)
            }

            override fun onTouchEvent(ev: MotionEvent): Boolean {
                if (closing) return true
                if (!dragging && ev.actionMasked == MotionEvent.ACTION_MOVE) {
                    val dy = ev.rawY - downY
                    val dx = ev.rawX - downX
                    if (dy > touchSlop && dy > kotlin.math.abs(dx) && !canSheetScrollUp(ev)) {
                        dragging = true
                        dragStartY = ev.rawY
                        animate().cancel()
                        scrimView.animate().cancel()
                    }
                }
                when (ev.actionMasked) {
                    MotionEvent.ACTION_DOWN -> {
                        downX = ev.rawX
                        downY = ev.rawY
                        dragStartY = ev.rawY
                        velocityTracker?.recycle()
                        velocityTracker = VelocityTracker.obtain().apply { addMovement(ev) }
                        return true
                    }
                    MotionEvent.ACTION_MOVE -> {
                        velocityTracker?.addMovement(ev)
                        if (dragging) {
                            val offset = (ev.rawY - dragStartY).coerceAtLeast(0f)
                            translationY = offset
                            val range = (height.coerceAtLeast(dp(200f)) * 0.9f)
                            scrimView.alpha = (1f - offset / range).coerceIn(0f, 1f)
                            return true
                        }
                    }
                    MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                        velocityTracker?.addMovement(ev)
                        velocityTracker?.computeCurrentVelocity(1000)
                        val vy = velocityTracker?.yVelocity ?: 0f
                        velocityTracker?.recycle()
                        velocityTracker = null
                        if (dragging) {
                            dragging = false
                            val threshold = (height * 0.28f).coerceAtMost(dp(120f).toFloat())
                            if (ev.actionMasked == MotionEvent.ACTION_UP &&
                                (translationY > threshold || vy > dismissVelocity)
                            ) {
                                closeNow()
                            } else {
                                val settleCurve = PathInterpolator(0.32f, 0.72f, 0f, 1f)
                                animate()
                                    .translationY(0f)
                                    .setDuration(200L)
                                    .setInterpolator(settleCurve)
                                    .start()
                                scrimView.animate()
                                    .alpha(1f)
                                    .setDuration(200L)
                                    .setInterpolator(settleCurve)
                                    .start()
                            }
                            return true
                        }
                    }
                }
                return super.onTouchEvent(ev)
            }
        }.apply {
            orientation = LinearLayout.VERTICAL
            background = roundedBg(cardBgColor(), 26f, cardBorderColor(), 1f)
            elevation = dp(18f).toFloat()
            setPadding(dp(18f), dp(12f), dp(18f), dp(18f))
            isClickable = true
            isFocusable = true
        }

        val handle = View(this).apply {
            background = roundedBg(if (darkMode) 0x4DFFFFFF else 0x38000000, 99f)
        }
        sheetCard.addView(
            handle,
            LinearLayout.LayoutParams(dp(36f), dp(4f)).apply {
                gravity = Gravity.CENTER_HORIZONTAL
                bottomMargin = dp(10f)
            },
        )

        val headerRow = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        val titleCol = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
        }
        val titleView = TextView(this).apply {
            text = "通过 Taildrop 发送"
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 16f)
            setTypeface(null, Typeface.BOLD)
            setTextColor(inkColor())
        }
        subtitleView = TextView(this).apply {
            text = "正在导入文件…"
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
            setTextColor(mutedColor())
            maxLines = 1
            ellipsize = TextUtils.TruncateAt.END
        }
        titleCol.addView(titleView)
        titleCol.addView(subtitleView)
        headerRow.addView(titleCol, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))

        val closeBtn = TextView(this).apply {
            text = "关闭"
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 14f)
            setTextColor(mutedColor())
            setPadding(dp(12f), dp(6f), dp(4f), dp(6f))
            setOnClickListener { closeNow() }
        }
        headerRow.addView(closeBtn)
        sheetCard.addView(
            headerRow,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                bottomMargin = dp(10f)
            },
        )

        val scrollContent = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
        }

        val filesCard = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            background = roundedBg(fillColor(), 14f, subtleBorderColor(), 0.75f)
            setPadding(dp(12f), dp(10f), dp(12f), dp(10f))
        }
        val filesTopRow = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        filesHeaderCount = TextView(this).apply {
            text = "正在读取文件…"
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
            setTypeface(null, Typeface.BOLD)
            setTextColor(inkColor())
        }
        filesHeaderSize = TextView(this).apply {
            text = ""
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
            typeface = Typeface.MONOSPACE
            setTextColor(mutedColor())
        }
        filesTopRow.addView(filesHeaderCount, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        filesTopRow.addView(filesHeaderSize)
        filesCard.addView(filesTopRow)

        filesListBox = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
        }
        filesCard.addView(
            filesListBox,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                topMargin = dp(4f)
            },
        )
        scrollContent.addView(
            filesCard,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT),
        )

        servicesChips = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        servicesRow = HorizontalScrollView(this).apply {
            isHorizontalScrollBarEnabled = false
            visibility = View.GONE
            addView(servicesChips)
        }
        scrollContent.addView(
            servicesRow,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                topMargin = dp(10f)
            },
        )

        endpointsChips = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        endpointsRow = HorizontalScrollView(this).apply {
            isHorizontalScrollBarEnabled = false
            visibility = View.GONE
            addView(endpointsChips)
        }
        scrollContent.addView(
            endpointsRow,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                topMargin = dp(8f)
            },
        )

        statusBox = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(2f), dp(14f), dp(2f), dp(6f))
        }
        statusSpinner = ProgressBar(this, null, android.R.attr.progressBarStyleSmall)
        statusLabel = TextView(this).apply {
            text = "正在连接 Tailscale…"
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 13f)
            setTextColor(mutedColor())
        }
        statusBox.addView(
            statusSpinner,
            LinearLayout.LayoutParams(dp(18f), dp(18f)).apply { rightMargin = dp(8f) },
        )
        statusBox.addView(statusLabel, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        scrollContent.addView(statusBox)

        peersHeader = TextView(this).apply {
            text = "选择接收设备"
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
            setTextColor(mutedColor())
            visibility = View.GONE
        }
        scrollContent.addView(
            peersHeader,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                topMargin = dp(12f)
                bottomMargin = dp(8f)
            },
        )

        peersListBox = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
        }
        scrollContent.addView(peersListBox)

        sheetScrollView = ScrollView(this).apply {
            isVerticalScrollBarEnabled = false
            addView(scrollContent)
        }
        val maxScrollHeight = (resources.displayMetrics.heightPixels * 0.56f).toInt()
        sheetCard.addView(
            sheetScrollView,
            LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
            ),
        )
        sheetScrollView.addOnLayoutChangeListener { v, _, _, _, _, _, _, _, _ ->
            if (v.height > maxScrollHeight) {
                val lp = v.layoutParams
                lp.height = maxScrollHeight
                v.layoutParams = lp
            }
        }

        sendButton = TextView(this).apply {
            text = "发送"
            gravity = Gravity.CENTER
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 14f)
            setTypeface(null, Typeface.BOLD)
            setTextColor(selectedInkColor())
            background = roundedBg(selectedBgColor(), 99f, selectedBorderColor(), 1f)
            visibility = View.GONE
            setOnClickListener { triggerSend() }
        }
        sheetCard.addView(
            sendButton,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(44f)).apply {
                topMargin = dp(12f)
            },
        )

        val cardParams = FrameLayout.LayoutParams(cardWidth, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
            gravity = Gravity.BOTTOM or Gravity.CENTER_HORIZONTAL
            bottomMargin = dp(12f)
            topMargin = dp(24f)
        }
        rootOverlay.addView(sheetCard, cardParams)
        setContentView(rootOverlay)

        ViewCompat.setOnApplyWindowInsetsListener(rootOverlay) { _, insets ->
            val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars())
            val lp = sheetCard.layoutParams as FrameLayout.LayoutParams
            lp.topMargin = bars.top + dp(16f)
            lp.bottomMargin = bars.bottom + dp(10f)
            sheetCard.layoutParams = lp
            insets
        }
    }

    private fun startShareFlow(incoming: Intent?) {
        staging = true
        connecting = true
        stageError = ""
        syncError = ""
        files.clear()
        endpoints = emptyList()
        selectedEndpointTag = null
        selectedPeerId = null
        render()

        if (incoming == null) {
            stageError = "未收到分享内容"
            staging = false
            connecting = false
            render()
            return
        }

        worker.execute {
            try {
                val staged = TaildropStaging.stageSendIntent(applicationContext, incoming)
                runOnUiThread {
                    if (isFinishing || isDestroyed) {
                        worker.execute { TaildropStaging.clearBatchDir(applicationContext, staged.batchDir) }
                        return@runOnUiThread
                    }
                    batchDir = staged.batchDir
                    files.clear()
                    files.addAll(staged.files)
                    staging = false
                    render()
                }
            } catch (e: Exception) {
                runOnUiThread {
                    if (isFinishing || isDestroyed) return@runOnUiThread
                    staging = false
                    stageError = e.localizedMessage ?: "无法读取分享内容"
                    render()
                }
            }
        }
        syncTailscale(null)
    }

    private fun syncTailscale(serviceTag: String?) {
        val epoch = connectEpoch.incrementAndGet()
        connecting = true
        syncError = ""
        if (serviceTag != null) {
            selectedServiceTag = serviceTag
            selectedEndpointTag = null
            selectedPeerId = null
        }
        render()

        worker.execute {
            try {
                val api = client ?: TaildropClient(filesDir.absolutePath).also { client = it }
                api.disconnect()
                val discovered = api.discover()
                if (connectEpoch.get() != epoch) return@execute

                if (discovered.isEmpty()) {
                    runOnUiThread {
                        if (connectEpoch.get() != epoch || isFinishing) return@runOnUiThread
                        services = emptyList()
                        endpoints = emptyList()
                        connecting = false
                        syncError = "尚未配置 Tailscale Service API"
                        render()
                    }
                    return@execute
                }

                val chosenTag = when {
                    selectedServiceTag != null && discovered.any { it.serviceTag == selectedServiceTag } ->
                        selectedServiceTag
                    discovered.size == 1 -> discovered.first().serviceTag
                    else -> null
                }

                if (chosenTag == null && discovered.size > 1) {
                    runOnUiThread {
                        if (connectEpoch.get() != epoch || isFinishing) return@runOnUiThread
                        services = discovered
                        endpoints = emptyList()
                        connecting = false
                        render()
                    }
                    return@execute
                }

                api.connect(chosenTag)
                val eps = api.statusEndpoints()
                if (connectEpoch.get() != epoch) return@execute

                runOnUiThread {
                    if (connectEpoch.get() != epoch || isFinishing) return@runOnUiThread
                    services = discovered
                    selectedServiceTag = chosenTag
                    endpoints = eps
                    if (eps.none { it.endpointTag == selectedEndpointTag }) {
                        selectedEndpointTag = eps.firstOrNull()?.endpointTag
                    }
                    val currentEp = eps.firstOrNull { it.endpointTag == selectedEndpointTag }
                    val prefs = getSharedPreferences("taildrop_share", MODE_PRIVATE)
                    val lastPeerId = prefs.getString("last_peer_id", null)
                    if (currentEp == null || currentEp.peers.none { it.stableId == selectedPeerId }) {
                        selectedPeerId = when {
                            lastPeerId != null && currentEp?.peers?.any { it.stableId == lastPeerId } == true ->
                                lastPeerId
                            else -> currentEp?.peers?.firstOrNull()?.stableId
                        }
                    }
                    connecting = false
                    render()
                }
            } catch (e: Exception) {
                runOnUiThread {
                    if (connectEpoch.get() != epoch || isFinishing) return@runOnUiThread
                    connecting = false
                    endpoints = emptyList()
                    syncError = e.localizedMessage ?: "无法连接 Tailscale Service API"
                    render()
                }
            }
        }
    }

    private fun currentEndpoint(): TaildropEndpoint? =
        endpoints.firstOrNull { it.endpointTag == selectedEndpointTag } ?: endpoints.firstOrNull()

    private fun currentPeer(ep: TaildropEndpoint?): TaildropPeer? =
        ep?.peers?.firstOrNull { it.stableId == selectedPeerId } ?: ep?.peers?.firstOrNull()

    private fun render() {
        val ep = currentEndpoint()
        val peer = currentPeer(ep)
        val effectiveError = stageError.ifEmpty { syncError }
        val nextMode = when {
            effectiveError.isNotEmpty() -> 1
            staging || connecting -> 2
            services.size > 1 && selectedServiceTag == null -> 3
            ep == null || ep.peers.isEmpty() -> 4
            else -> 5
        }
        if (lastVisualMode != -1 && lastVisualMode != nextMode && !closing && sheetCard.isLaidOut) {
            TransitionManager.beginDelayedTransition(
                rootOverlay,
                AutoTransition().apply {
                    duration = 200L
                    interpolator = PathInterpolator(0.32f, 0.72f, 0f, 1f)
                },
            )
        }
        lastVisualMode = nextMode

        subtitleView.text = when {
            staging -> "正在导入文件…"
            ep != null -> listOf(ep.endpointTag, ep.networkName).filter { it.isNotEmpty() }.joinToString(" · ")
            connecting -> "正在连接 Tailscale…"
            services.size > 1 && selectedServiceTag == null -> "发现 ${services.size} 个服务"
            else -> "Tailscale"
        }

        val totalBytes = files.sumOf { it.size }
        filesHeaderCount.text = if (staging) "正在读取文件…" else "已接收 ${files.size} 个文件"
        filesHeaderSize.text = if (staging) "" else TaildropStaging.formatBytes(totalBytes)
        filesListBox.removeAllViews()
        for (i in files.indices) {
            val item = files[i]
            val row = LinearLayout(this).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(0, dp(4f), 0, 0)
            }
            val nameView = TextView(this).apply {
                text = item.name
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
                setTextColor(inkColor())
                maxLines = 1
                ellipsize = TextUtils.TruncateAt.MIDDLE
            }
            val sizeView = TextView(this).apply {
                text = TaildropStaging.formatBytes(item.size)
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 11f)
                typeface = Typeface.MONOSPACE
                setTextColor(mutedColor())
            }
            row.addView(nameView, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
            row.addView(
                sizeView,
                LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                    leftMargin = dp(8f)
                },
            )
            if (files.size > 1) {
                val removeView = TextView(this).apply {
                    text = "×"
                    setTextSize(TypedValue.COMPLEX_UNIT_SP, 15f)
                    setTextColor(mutedColor())
                    setPadding(dp(8f), 0, dp(2f), 0)
                    setOnClickListener {
                        files.removeAt(i)
                        render()
                    }
                }
                row.addView(removeView)
            }
            filesListBox.addView(row)
        }

        servicesChips.removeAllViews()
        if (services.size > 1) {
            servicesRow.visibility = View.VISIBLE
            for (svc in services) {
                val selected = svc.serviceTag == selectedServiceTag
                servicesChips.addView(
                    makePillChip(svc.serviceTag.ifEmpty { svc.url }, selected) {
                        if (!connecting) syncTailscale(svc.serviceTag)
                    },
                )
            }
        } else {
            servicesRow.visibility = View.GONE
        }

        endpointsChips.removeAllViews()
        if (endpoints.size > 1) {
            endpointsRow.visibility = View.VISIBLE
            for (item in endpoints) {
                val selected = item.endpointTag == ep?.endpointTag
                endpointsChips.addView(
                    makePillChip(item.endpointTag, selected) {
                        selectedEndpointTag = item.endpointTag
                        selectedPeerId = item.peers.firstOrNull()?.stableId
                        render()
                    },
                )
            }
        } else {
            endpointsRow.visibility = View.GONE
        }

        peersListBox.removeAllViews()
        when {
            effectiveError.isNotEmpty() -> {
                statusBox.visibility = View.VISIBLE
                statusSpinner.visibility = View.GONE
                statusLabel.text = effectiveError
                statusLabel.setTextColor(dangerColor())
                peersHeader.visibility = View.GONE
                sendButton.visibility = View.GONE
            }
            staging || connecting -> {
                statusBox.visibility = View.VISIBLE
                statusSpinner.visibility = View.VISIBLE
                statusLabel.text = if (staging) "正在导入分享文件…" else "正在获取接收设备…"
                statusLabel.setTextColor(mutedColor())
                peersHeader.visibility = View.GONE
                sendButton.visibility = View.GONE
            }
            services.size > 1 && selectedServiceTag == null -> {
                statusBox.visibility = View.VISIBLE
                statusSpinner.visibility = View.GONE
                statusLabel.text = "请选择上方的 Service API 服务。"
                statusLabel.setTextColor(mutedColor())
                peersHeader.visibility = View.GONE
                sendButton.visibility = View.GONE
            }
            ep == null || ep.peers.isEmpty() -> {
                statusBox.visibility = View.VISIBLE
                statusSpinner.visibility = View.GONE
                statusLabel.text = if (ep == null) {
                    "当前服务没有可发送文件的端点。"
                } else {
                    "当前 Tailnet 中没有可接收 Taildrop 文件的设备。"
                }
                statusLabel.setTextColor(mutedColor())
                peersHeader.visibility = View.GONE
                sendButton.visibility = View.GONE
            }
            else -> {
                statusBox.visibility = View.GONE
                peersHeader.visibility = View.VISIBLE
                for (target in ep.peers) {
                    val selected = target.stableId == peer?.stableId
                    peersListBox.addView(makePeerRow(target, selected))
                }
                sendButton.visibility = View.VISIBLE
                sendButton.isEnabled = peer != null && files.isNotEmpty()
                sendButton.alpha = if (sendButton.isEnabled) 1f else 0.45f
                sendButton.text = "发送到 ${peer?.displayName ?: "设备"}"
            }
        }
    }

    private fun makePillChip(label: String, selected: Boolean, onTap: () -> Unit): View =
        TextView(this).apply {
            text = label
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
            setTypeface(null, if (selected) Typeface.BOLD else Typeface.NORMAL)
            setTextColor(if (selected) selectedInkColor() else inkColor())
            background = roundedBg(
                if (selected) selectedBgColor() else fillColor(),
                99f,
                if (selected) selectedBorderColor() else subtleBorderColor(),
                0.75f,
            )
            setPadding(dp(12f), dp(6f), dp(12f), dp(6f))
            layoutParams = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
            ).apply {
                rightMargin = dp(6f)
            }
            setOnClickListener { onTap() }
        }

    private fun makePeerRow(peer: TaildropPeer, selected: Boolean): View {
        val row = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            background = roundedBg(
                if (selected) selectedBgColor() else fillColor(),
                14f,
                if (selected) selectedBorderColor() else subtleBorderColor(),
                0.75f,
            )
            setPadding(dp(12f), dp(10f), dp(12f), dp(10f))
            setOnClickListener {
                selectedPeerId = peer.stableId
                render()
            }
        }
        val dot = View(this).apply {
            background = roundedBg(if (peer.online) successColor() else mutedColor(), 99f)
        }
        row.addView(
            dot,
            LinearLayout.LayoutParams(dp(8f), dp(8f)).apply { rightMargin = dp(10f) },
        )
        val infoCol = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        val nameView = TextView(this).apply {
            text = peer.displayName
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 13.5f)
            setTypeface(null, Typeface.BOLD)
            setTextColor(if (selected) selectedInkColor() else inkColor())
            maxLines = 1
            ellipsize = TextUtils.TruncateAt.END
        }
        val metaText = listOf(peer.os, peer.primaryIp).filter { it.isNotEmpty() }.joinToString(" · ")
        val metaView = TextView(this).apply {
            text = metaText
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 11f)
            typeface = Typeface.MONOSPACE
            setTextColor(if (selected) selectedInkColor() else mutedColor())
            alpha = if (selected) 0.78f else 1f
            maxLines = 1
            ellipsize = TextUtils.TruncateAt.END
        }
        infoCol.addView(nameView)
        if (metaText.isNotEmpty()) infoCol.addView(metaView)
        row.addView(infoCol, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))

        if (selected) {
            val checkBadge = TextView(this).apply {
                text = "✓"
                gravity = Gravity.CENTER
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
                setTypeface(null, Typeface.BOLD)
                setTextColor(selectedInkColor())
                background = roundedBg(if (darkMode) 0x26FFFFFF else 0x1F000000, 99f)
            }
            row.addView(
                checkBadge,
                LinearLayout.LayoutParams(dp(20f), dp(20f)).apply { leftMargin = dp(8f) },
            )
        }

        row.layoutParams = LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            ViewGroup.LayoutParams.WRAP_CONTENT,
        ).apply {
            bottomMargin = dp(6f)
        }
        return row
    }

    private fun triggerSend() {
        val ep = currentEndpoint() ?: return
        val peer = currentPeer(ep) ?: return
        if (batchDir.isEmpty() || files.isEmpty() || handedOff || closing) return
        handedOff = true
        getSharedPreferences("taildrop_share", MODE_PRIVATE)
            .edit()
            .putString("last_peer_id", peer.stableId)
            .apply()
        val paths = files.map { it.path }
        val totalBytes = files.sumOf { it.size }
        try {
            TaildropTransferService.enqueue(
                context = applicationContext,
                batchDir = batchDir,
                paths = paths,
                totalBytes = totalBytes,
                serviceTag = selectedServiceTag,
                endpointTag = ep.endpointTag,
                peerId = peer.stableId,
                peerName = peer.displayName,
            )
            Toast.makeText(applicationContext, "正在后台发送到 ${peer.displayName}", Toast.LENGTH_SHORT).show()
        } catch (e: Exception) {
            handedOff = false
            Toast.makeText(applicationContext, "无法启动传输：${e.localizedMessage}", Toast.LENGTH_LONG).show()
            return
        }
        closeNow()
    }
}
