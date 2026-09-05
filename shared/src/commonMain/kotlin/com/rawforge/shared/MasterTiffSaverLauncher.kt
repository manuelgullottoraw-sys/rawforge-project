package com.rawforge.shared

import androidx.compose.runtime.Composable

/**
 * Come `rememberFileSaverLauncher`, ma per il master TIFF a 16 bit senza
 * perdita (`FullResolutionExport.masterTiffBytes`) invece del JPEG di
 * consegna: stesso principio (selettore nativo di destinazione), ma con un
 * tipo di contenuto TIFF invece che JPEG — riusare `rememberFileSaverLauncher`
 * qui proporrebbe su Android il MIME "image/jpeg" per un file che non lo è
 * (lo stesso tipo di bug MIME/estensione già corretto una volta in questo
 * progetto per l'esportazione JPEG/PNG). Aggiunto in questo giro insieme al
 * master TIFF stesso: prima l'esportazione a piena risoluzione produceva un
 * solo file.
 */
@Composable
expect fun rememberMasterTiffSaverLauncher(
    onSaved: (destination: String) -> Unit,
    onError: (String) -> Unit,
): (bytes: ByteArray, suggestedFileName: String) -> Unit
