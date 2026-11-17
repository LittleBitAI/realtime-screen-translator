package ai.littlebit.realtime_screen_translator

import android.os.Bundle
import android.view.View
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // Android 15 draws every app edge to edge, so the screens would sit under
    // the status bar and the navigation buttons. Keep them inside the bars,
    // the display cutout and the keyboard.
    val sides = WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout() or WindowInsetsCompat.Type.ime()
    ViewCompat.setOnApplyWindowInsetsListener(findViewById<View>(android.R.id.content)) { view, insets ->
      val pad = insets.getInsets(sides)
      view.setPadding(pad.left, pad.top, pad.right, pad.bottom)
      WindowInsetsCompat.CONSUMED
    }
  }
}
