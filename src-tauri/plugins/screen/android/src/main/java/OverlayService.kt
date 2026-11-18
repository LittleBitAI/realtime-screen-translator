package ai.littlebit.rst.screen

import android.accessibilityservice.AccessibilityService
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.provider.Settings
import android.view.accessibility.AccessibilityEvent

/**
 * Only lends the overlay a trusted window type. Android 12+ holds back a
 * touch that passes through an opaque window of another app unless that
 * window is an accessibility overlay; with this service on, the white
 * shapes are fully opaque and every touch still reaches the reader's app.
 * It reads no screen content and acts on no event (the capture stays
 * MediaProjection, A2).
 */
class OverlayService : AccessibilityService() {
    override fun onServiceConnected() {
        running = this
        Overlay.serviceChanged()
    }

    override fun onUnbind(intent: Intent?): Boolean {
        running = null
        Overlay.serviceChanged()
        return super.onUnbind(intent)
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {}

    override fun onInterrupt() {}

    companion object {
        @Volatile var running: OverlayService? = null

        /** Whether the person switched the service on in the accessibility settings. */
        fun enabled(ctx: Context): Boolean {
            val list = Settings.Secure.getString(ctx.contentResolver, Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES) ?: return false
            val me = ComponentName(ctx, OverlayService::class.java).flattenToString()
            return list.split(':').any { it.equals(me, ignoreCase = true) }
        }
    }
}
