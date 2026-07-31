package com.dripai.shiping.ui

import android.os.Build
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.dripai.shiping.recording.AudioMode
import com.dripai.shiping.recording.RecordingConfig
import com.dripai.shiping.recording.RecordingPhase
import com.dripai.shiping.recording.RecordingUiState
import com.dripai.shiping.recording.VideoQuality
import kotlinx.coroutines.flow.StateFlow
import java.util.Locale

private enum class AppTab(val label: String, val marker: String) {
    Record("录制", "●"),
    History("记录", "▤"),
    Settings("设置", "⚙"),
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ShiPingApp(
    state: StateFlow<RecordingUiState>,
    onStart: (RecordingConfig) -> Unit,
    onStop: () -> Unit,
) {
    val uiState by state.collectAsStateWithLifecycle()
    var selectedTab by remember { mutableStateOf(AppTab.Record) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Column {
                        Text("ShiPing", fontWeight = FontWeight.SemiBold)
                        Text(
                            "轻量、直接的屏幕录制",
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                },
            )
        },
        bottomBar = {
            NavigationBar {
                AppTab.entries.forEach { tab ->
                    NavigationBarItem(
                        selected = selectedTab == tab,
                        onClick = { selectedTab = tab },
                        icon = { Text(tab.marker, fontSize = 19.sp) },
                        label = { Text(tab.label) },
                    )
                }
            }
        },
    ) { padding ->
        when (selectedTab) {
            AppTab.Record -> RecordingScreen(
                uiState = uiState,
                onStart = onStart,
                onStop = onStop,
                contentPadding = padding,
            )
            AppTab.History -> HistoryScreen(uiState, padding)
            AppTab.Settings -> SettingsScreen(padding)
        }
    }
}

@Composable
private fun RecordingScreen(
    uiState: RecordingUiState,
    onStart: (RecordingConfig) -> Unit,
    onStop: () -> Unit,
    contentPadding: PaddingValues,
) {
    var quality by remember { mutableStateOf(VideoQuality.FullHd) }
    var frameRate by remember { mutableIntStateOf(30) }
    var audioMode by remember {
        mutableStateOf(if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) AudioMode.System else AudioMode.Microphone)
    }

    val isRecording = uiState.phase == RecordingPhase.Recording
    val canConfigure = !uiState.isActive

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(contentPadding)
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp, vertical = 12.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(
            text = formatElapsed(uiState.elapsedMs),
            style = MaterialTheme.typography.displayLarge,
            fontWeight = FontWeight.Light,
        )
        Spacer(Modifier.height(8.dp))
        StatusPill(uiState)
        Spacer(Modifier.height(28.dp))

        RecordButton(
            active = isRecording,
            enabled = uiState.phase != RecordingPhase.Authorizing &&
                uiState.phase != RecordingPhase.Finalizing,
            onClick = {
                if (isRecording) {
                    onStop()
                } else {
                    onStart(RecordingConfig(quality, frameRate, audioMode))
                }
            },
        )
        Spacer(Modifier.height(10.dp))
        Text(
            text = if (isRecording) "点击停止并保存" else "点击开始录制",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(28.dp))

        SettingsCard(
            quality = quality,
            onQualityChanged = { if (canConfigure) quality = it },
            frameRate = frameRate,
            onFrameRateChanged = { if (canConfigure) frameRate = it },
            audioMode = audioMode,
            onAudioModeChanged = { if (canConfigure) audioMode = it },
            enabled = canConfigure,
        )

        if (uiState.outputUri != null) {
            Spacer(Modifier.height(16.dp))
            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.secondaryContainer,
                ),
            ) {
                Column(Modifier.padding(16.dp)) {
                    Text("最近一次录制", fontWeight = FontWeight.SemiBold)
                    Spacer(Modifier.height(6.dp))
                    Text(
                        uiState.outputUri,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSecondaryContainer,
                    )
                }
            }
        }
        Spacer(Modifier.height(24.dp))
    }
}

@Composable
private fun StatusPill(uiState: RecordingUiState) {
    val color = when (uiState.phase) {
        RecordingPhase.Recording -> Color(0xFF24A66A)
        RecordingPhase.Failed -> MaterialTheme.colorScheme.error
        RecordingPhase.Authorizing, RecordingPhase.Finalizing -> MaterialTheme.colorScheme.tertiary
        else -> MaterialTheme.colorScheme.onSurfaceVariant
    }

    Surface(
        shape = RoundedCornerShape(100),
        color = color.copy(alpha = 0.12f),
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 14.dp, vertical = 7.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                Modifier
                    .size(7.dp)
                    .background(color, CircleShape),
            )
            Spacer(Modifier.width(8.dp))
            Text(uiState.message, color = color, style = MaterialTheme.typography.labelLarge)
        }
    }
}

@Composable
private fun RecordButton(
    active: Boolean,
    enabled: Boolean,
    onClick: () -> Unit,
) {
    val outerColor = if (active) {
        MaterialTheme.colorScheme.errorContainer
    } else {
        MaterialTheme.colorScheme.primaryContainer
    }
    val innerColor = if (active) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary

    Surface(
        onClick = onClick,
        enabled = enabled,
        shape = CircleShape,
        color = outerColor,
        modifier = Modifier.size(126.dp),
    ) {
        Box(contentAlignment = Alignment.Center) {
            Box(
                modifier = Modifier
                    .size(if (active) 42.dp else 58.dp)
                    .background(
                        color = innerColor,
                        shape = if (active) RoundedCornerShape(12.dp) else CircleShape,
                    ),
            )
        }
    }
}

@Composable
private fun SettingsCard(
    quality: VideoQuality,
    onQualityChanged: (VideoQuality) -> Unit,
    frameRate: Int,
    onFrameRateChanged: (Int) -> Unit,
    audioMode: AudioMode,
    onAudioModeChanged: (AudioMode) -> Unit,
    enabled: Boolean,
) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(24.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.72f),
        ),
    ) {
        Column(Modifier.padding(18.dp)) {
            Text("录制参数", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.height(16.dp))

            OptionGroup("画质") {
                VideoQuality.entries.forEach { option ->
                    FilterChip(
                        selected = quality == option,
                        onClick = { onQualityChanged(option) },
                        label = { Text(option.label) },
                        enabled = enabled,
                    )
                }
            }

            HorizontalDivider(Modifier.padding(vertical = 14.dp))

            OptionGroup("帧率") {
                listOf(30, 60).forEach { option ->
                    FilterChip(
                        selected = frameRate == option,
                        onClick = { onFrameRateChanged(option) },
                        label = { Text("$option FPS") },
                        enabled = enabled,
                    )
                }
            }

            HorizontalDivider(Modifier.padding(vertical = 14.dp))

            OptionGroup("声音来源") {
                AudioMode.entries.forEach { option ->
                    val supported = option != AudioMode.System ||
                        Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q
                    FilterChip(
                        selected = audioMode == option,
                        onClick = { onAudioModeChanged(option) },
                        label = { Text(option.label) },
                        enabled = enabled && supported,
                    )
                }
            }
            Spacer(Modifier.height(8.dp))
            Text(
                "系统声音和麦克风第一版互斥；部分应用会禁止系统音频被捕获。",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun OptionGroup(
    title: String,
    content: @Composable () -> Unit,
) {
    Text(
        title,
        style = MaterialTheme.typography.labelLarge,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
    Spacer(Modifier.height(8.dp))
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        content()
    }
}

@Composable
private fun HistoryScreen(
    uiState: RecordingUiState,
    padding: PaddingValues,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(padding)
            .padding(24.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("录制记录", style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold)
        Spacer(Modifier.height(12.dp))
        Text(
            uiState.outputUri ?: "完成录制后，视频会保存到 Movies/ShiPing。",
            textAlign = TextAlign.Center,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun SettingsScreen(padding: PaddingValues) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(padding)
            .padding(24.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("ShiPing Android", style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold)
        Spacer(Modifier.height(10.dp))
        Text(
            "Kotlin + Material 3 UI\nRust 共享状态核心\nAndroid 原生录屏管线",
            textAlign = TextAlign.Center,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

private fun formatElapsed(elapsedMs: Long): String {
    val totalSeconds = elapsedMs / 1_000
    val hours = totalSeconds / 3_600
    val minutes = (totalSeconds % 3_600) / 60
    val seconds = totalSeconds % 60
    return String.format(Locale.ROOT, "%02d:%02d:%02d", hours, minutes, seconds)
}
