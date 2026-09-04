package com.rawforge.shared

import androidx.compose.ui.graphics.ImageBitmap

/**
 * Decodifica bytes di un'immagine (PNG o JPEG) in un `ImageBitmap` Compose,
 * usando il decoder nativo di ciascuna piattaforma (Skia su Desktop,
 * `BitmapFactory` su Android — non serve un decoder scritto a mano: sono già
 * gli stessi decoder che disegnano ogni altra immagine nell'app). Ritorna
 * `null`, invece di lanciare un'eccezione, se i bytes non sono un'immagine
 * valida — protegge la UI da un crash senza bloccare il resto del flusso di
 * importazione (i metadati camera restano comunque visibili).
 */
expect fun decodeImageBitmapOrNull(bytes: ByteArray): ImageBitmap?
