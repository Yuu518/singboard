package io.github.Yuu518.singboard

import android.app.Activity
import android.content.Intent
import android.graphics.Color
import android.graphics.drawable.ColorDrawable
import android.net.ConnectivityManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
import android.view.View
import android.webkit.WebView
import androidx.core.content.FileProvider
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.util.Locale

@InvokeArg
class InstallApkArgs {
  lateinit var path: String
}

@InvokeArg
class OpenUrlArgs {
  lateinit var url: String
}

@InvokeArg
class SystemBarsArgs {
  var dark: Boolean = false
  var color: String? = null
}

@TauriPlugin
class SingboardPlugin(private val activity: Activity) : Plugin(activity) {
  private var webView: WebView? = null
  private val insets = floatArrayOf(0f, 0f, 0f, 0f)
  private var keyboardInset = 0f

  override fun load(webView: WebView) {
    this.webView = webView
    activity.runOnUiThread {
      val content = activity.findViewById<View>(android.R.id.content)
      ViewCompat.setOnApplyWindowInsetsListener(content) { _, windowInsets ->
        val bars = windowInsets.getInsets(
          WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
        )
        val ime = windowInsets.getInsets(WindowInsetsCompat.Type.ime())
        publishInsets(bars.top, bars.right, bars.bottom, bars.left)
        publishKeyboard(if (ime.bottom > bars.bottom) ime.bottom else 0)
        WindowInsetsCompat.CONSUMED
      }
      ViewCompat.requestApplyInsets(content)
    }
  }

  private fun publishInsets(top: Int, right: Int, bottom: Int, left: Int) {
    val density = activity.resources.displayMetrics.density
    insets[0] = top / density
    insets[1] = right / density
    insets[2] = bottom / density
    insets[3] = left / density
    val names = arrayOf("top", "right", "bottom", "left")
    val script = StringBuilder("(function(s){")
    for (i in names.indices) {
      script.append(String.format(Locale.US, "s.setProperty('--safe-%s','%.2fpx');", names[i], insets[i]))
    }
    script.append("})(document.documentElement.style)")
    webView?.evaluateJavascript(script.toString(), null)
  }

  private fun publishKeyboard(bottom: Int) {
    val inset = bottom / activity.resources.displayMetrics.density
    if (inset == keyboardInset) return
    keyboardInset = inset
    val script = String.format(
      Locale.US,
      "window.dispatchEvent(new CustomEvent('singboard-keyboard',{detail:%.2f}))",
      inset
    )
    webView?.evaluateJavascript(script, null)
  }

  @Command
  fun systemInsets(invoke: Invoke) {
    activity.runOnUiThread {
      val ret = JSObject()
      ret.put("top", insets[0].toDouble())
      ret.put("right", insets[1].toDouble())
      ret.put("bottom", insets[2].toDouble())
      ret.put("left", insets[3].toDouble())
      invoke.resolve(ret)
    }
  }

  @Command
  fun apkInfo(invoke: Invoke) {
    val ret = JSObject()
    ret.put("path", activity.applicationInfo.sourceDir)
    invoke.resolve(ret)
  }

  @Command
  fun dnsServers(invoke: Invoke) {
    val servers = JSArray()
    val manager = activity.getSystemService(ConnectivityManager::class.java)
    val network = manager?.activeNetwork
    val properties = if (network != null) manager.getLinkProperties(network) else null
    properties?.dnsServers?.forEach { address ->
      address.hostAddress?.let { servers.put(it) }
    }
    val ret = JSObject()
    ret.put("servers", servers)
    invoke.resolve(ret)
  }

  @Command
  fun installApk(invoke: Invoke) {
    val args = invoke.parseArgs(InstallApkArgs::class.java)
    val file = File(args.path)
    if (!file.isFile) {
      invoke.reject("安装包不存在")
      return
    }
    if (!activity.packageManager.canRequestPackageInstalls()) {
      val settings = Intent(
        Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
        Uri.parse("package:${activity.packageName}")
      )
      activity.startActivity(settings)
      invoke.reject("install_permission_required")
      return
    }
    val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", file)
    val intent = Intent(Intent.ACTION_VIEW).apply {
      setDataAndType(uri, "application/vnd.android.package-archive")
      addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
    }
    activity.startActivity(intent)
    invoke.resolve(JSObject())
  }

  @Command
  fun openUrl(invoke: Invoke) {
    val args = invoke.parseArgs(OpenUrlArgs::class.java)
    val intent = Intent(Intent.ACTION_VIEW, Uri.parse(args.url)).apply {
      addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    }
    try {
      activity.startActivity(intent)
      invoke.resolve(JSObject())
    } catch (e: Exception) {
      invoke.reject("无法打开浏览器")
    }
  }

  @Command
  fun setSystemBars(invoke: Invoke) {
    val args = invoke.parseArgs(SystemBarsArgs::class.java)
    activity.runOnUiThread {
      val window = activity.window
      val controller = WindowCompat.getInsetsController(window, window.decorView)
      controller.isAppearanceLightStatusBars = !args.dark
      controller.isAppearanceLightNavigationBars = !args.dark
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
        window.isStatusBarContrastEnforced = false
        window.isNavigationBarContrastEnforced = false
      }
      args.color?.let { runCatching { Color.parseColor(it) }.getOrNull() }?.let { color ->
        window.setBackgroundDrawable(ColorDrawable(color))
        activity.findViewById<View>(android.R.id.content).setBackgroundColor(color)
        webView?.setBackgroundColor(color)
      }
      invoke.resolve(JSObject())
    }
  }
}
