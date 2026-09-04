package com.rawforge.shared

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.Button
import androidx.compose.material.MaterialTheme
import androidx.compose.material.Surface
import androidx.compose.material.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

/**
 * UI condivisa (identica su Android e Windows). I due pulsanti chiamano per
 * davvero il motore Rust attraverso i binding UniFFI (`Engine`, vedi Engine.kt):
 * non sono più placeholder. Il modulo Develop, la libreria a griglia e lo
 * Smart-Batch (docs/ARCHITECTURE.md, §4) restano da costruire sopra questa base.
 */
@Composable
fun RawForgeApp() {
    var engineInfo by remember { mutableStateOf<String?>(null) }
    var xmpPreview by remember { mutableStateOf<String?>(null) }

    MaterialTheme {
        Surface(modifier = Modifier.fillMaxSize()) {
            Column(
                modifier = Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center
            ) {
                Text("RawForge", style = MaterialTheme.typography.h3)
                Spacer(Modifier.height(8.dp))
                Text("Motore RAW ultra-veloce — motore Rust collegato via UniFFI")
                Spacer(Modifier.height(24.dp))

                Button(onClick = { engineInfo = Engine.versionInfo() }) {
                    Text("Stato motore")
                }
                engineInfo?.let {
                    Spacer(Modifier.height(8.dp))
                    Text(it)
                }

                Spacer(Modifier.height(24.dp))

                Button(onClick = { xmpPreview = Engine.generateSampleXmpPreset() }) {
                    Text("Genera preset XMP di esempio (Sintesi Armonica)")
                }
                xmpPreview?.let {
                    Spacer(Modifier.height(8.dp))
                    Text(
                        it.take(600) + if (it.length > 600) "\n… (troncato)" else "",
                        modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp),
                        style = MaterialTheme.typography.caption
                    )
                }
            }
        }
    }
}
