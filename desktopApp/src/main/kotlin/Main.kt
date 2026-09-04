import androidx.compose.ui.window.Window
import androidx.compose.ui.window.application
import com.rawforge.shared.RawForgeApp

fun main() = application {
    Window(onCloseRequest = ::exitApplication, title = "RawForge") {
        RawForgeApp()
    }
}
