package com.dripai.shiping.ui

import android.os.Build
import android.text.format.Formatter
import androidx.annotation.DrawableRes
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.dripai.shiping.BuildConfig
import com.dripai.shiping.R
import com.dripai.shiping.recording.AudioMode
import com.dripai.shiping.recording.RecordingConfig
import com.dripai.shiping.recording.RecordingDetails
import com.dripai.shiping.recording.RecordingItem
import com.dripai.shiping.recording.RecordingPhase
import com.dripai.shiping.recording.RecordingUiState
import com.dripai.shiping.recording.VideoQuality
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import java.text.DateFormat
import java.util.Date
import java.util.Locale

private enum class AppTab(
    val label: String,
    @DrawableRes val icon: Int,
) {
    Record("录制", R.drawable.ic_nav_record),
    History("记录", R.drawable.ic_nav_history),
    About("关于", R.drawable.ic_nav_about),
}

@Composable
fun ShiPingApp(
    state: StateFlow<RecordingUiState>,
    onStart: (RecordingConfig) -> Unit,
    onStop: () -> Unit,
    onLoadRecordings: suspend () -> List<RecordingItem>,
    onRenameRecording: suspend (RecordingItem, String) -> RecordingItem,
    onDeleteRecording: suspend (RecordingItem) -> Unit,
    onLoadRecordingDetails: suspend (RecordingItem) -> RecordingDetails,
    canOpenRecording: (RecordingItem) -> Boolean,
    onOpenRecording: (RecordingItem) -> Boolean,
) {
    val uiState by state.collectAsStateWithLifecycle()
    var selectedTab by remember { mutableStateOf(AppTab.Record) }

    Scaffold(
        bottomBar = {
            NavigationBar {
                AppTab.entries.forEach { tab ->
                    NavigationBarItem(
                        selected = selectedTab == tab,
                        onClick = { selectedTab = tab },
                        icon = {
                            Icon(
                                painter = painterResource(tab.icon),
                                contentDescription = tab.label,
                            )
                        },
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
            AppTab.History -> HistoryScreen(
                uiState = uiState,
                padding = padding,
                onLoadRecordings = onLoadRecordings,
                onRenameRecording = onRenameRecording,
                onDeleteRecording = onDeleteRecording,
                onLoadRecordingDetails = onLoadRecordingDetails,
                canOpenRecording = canOpenRecording,
                onOpenRecording = onOpenRecording,
            )
            AppTab.About -> AboutScreen(padding)
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
            .padding(horizontal = 16.dp, vertical = 8.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(
            text = formatElapsed(uiState.elapsedMs),
            style = MaterialTheme.typography.displayMedium,
            fontWeight = FontWeight.Light,
        )
        Spacer(Modifier.height(6.dp))
        StatusPill(uiState)
        Spacer(Modifier.height(16.dp))

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
        Spacer(Modifier.height(8.dp))
        Text(
            text = if (isRecording) "点击停止并保存" else "点击开始录制",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(18.dp))

        SettingsCard(
            quality = quality,
            onQualityChanged = { if (canConfigure) quality = it },
            frameRate = frameRate,
            onFrameRateChanged = { if (canConfigure) frameRate = it },
            audioMode = audioMode,
            onAudioModeChanged = { if (canConfigure) audioMode = it },
            enabled = canConfigure,
        )
        Spacer(Modifier.height(16.dp))
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
        modifier = Modifier.size(104.dp),
    ) {
        Box(contentAlignment = Alignment.Center) {
            Box(
                modifier = Modifier
                    .size(if (active) 36.dp else 50.dp)
                    .background(
                        color = innerColor,
                        shape = if (active) RoundedCornerShape(10.dp) else CircleShape,
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
        shape = RoundedCornerShape(22.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.72f),
        ),
    ) {
        Column(Modifier.padding(horizontal = 16.dp, vertical = 14.dp)) {
            Text("录制参数", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.height(10.dp))

            SettingRow("画质") {
                VideoQuality.entries.forEach { option ->
                    CompactFilterChip(
                        selected = quality == option,
                        onClick = { onQualityChanged(option) },
                        label = when (option) {
                            VideoQuality.Original -> "原始"
                            else -> option.label
                        },
                        enabled = enabled,
                    )
                }
            }

            SettingRow("帧率") {
                listOf(30, 60).forEach { option ->
                    CompactFilterChip(
                        selected = frameRate == option,
                        onClick = { onFrameRateChanged(option) },
                        label = "$option FPS",
                        enabled = enabled,
                    )
                }
                Spacer(Modifier.weight(1f))
            }

            SettingRow("声音来源") {
                AudioMode.entries.forEach { option ->
                    val supported = option != AudioMode.System ||
                        Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q
                    CompactFilterChip(
                        selected = audioMode == option,
                        onClick = { onAudioModeChanged(option) },
                        label = when (option) {
                            AudioMode.None -> "无声音"
                            AudioMode.System -> "系统"
                            AudioMode.Microphone -> "麦克风"
                        },
                        enabled = enabled && supported,
                    )
                }
            }
            Spacer(Modifier.height(4.dp))
            Text(
                "系统声音和麦克风暂时互斥；部分应用会禁止系统音频被捕获。",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun SettingRow(
    label: String,
    options: @Composable RowScope.() -> Unit,
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            label,
            modifier = Modifier.width(66.dp),
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Row(
            modifier = Modifier.weight(1f),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalAlignment = Alignment.CenterVertically,
            content = options,
        )
    }
}

@Composable
private fun RowScope.CompactFilterChip(
    selected: Boolean,
    onClick: () -> Unit,
    label: String,
    enabled: Boolean,
) {
    FilterChip(
        selected = selected,
        onClick = onClick,
        label = {
            Text(
                label,
                modifier = Modifier.fillMaxWidth(),
                textAlign = TextAlign.Center,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                style = MaterialTheme.typography.labelMedium,
            )
        },
        enabled = enabled,
        modifier = Modifier.weight(1f),
    )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun HistoryScreen(
    uiState: RecordingUiState,
    padding: PaddingValues,
    onLoadRecordings: suspend () -> List<RecordingItem>,
    onRenameRecording: suspend (RecordingItem, String) -> RecordingItem,
    onDeleteRecording: suspend (RecordingItem) -> Unit,
    onLoadRecordingDetails: suspend (RecordingItem) -> RecordingDetails,
    canOpenRecording: (RecordingItem) -> Boolean,
    onOpenRecording: (RecordingItem) -> Boolean,
) {
    var recordings by remember { mutableStateOf(emptyList<RecordingItem>()) }
    var loading by remember { mutableStateOf(true) }
    var loadError by remember { mutableStateOf<String?>(null) }
    var reloadVersion by remember { mutableIntStateOf(0) }
    var renameTarget by remember { mutableStateOf<RecordingItem?>(null) }
    var deleteTarget by remember { mutableStateOf<RecordingItem?>(null) }
    var detailsTarget by remember { mutableStateOf<RecordingItem?>(null) }
    var recordingDetails by remember { mutableStateOf<RecordingDetails?>(null) }
    var detailsLoading by remember { mutableStateOf(false) }
    var operationError by remember { mutableStateOf<String?>(null) }
    val coroutineScope = rememberCoroutineScope()

    LaunchedEffect(uiState.outputUri, reloadVersion) {
        loading = true
        loadError = null
        try {
            recordings = onLoadRecordings()
        } catch (error: Exception) {
            loadError = error.message ?: "无法读取录制记录"
        } finally {
            loading = false
        }
    }

    LaunchedEffect(detailsTarget?.uri) {
        val target = detailsTarget ?: return@LaunchedEffect
        detailsLoading = true
        recordingDetails = null
        try {
            recordingDetails = onLoadRecordingDetails(target)
        } catch (error: Exception) {
            operationError = error.readableMessage("无法读取录像详情")
            detailsTarget = null
        } finally {
            detailsLoading = false
        }
    }

    LazyColumn(
        modifier = Modifier
            .fillMaxSize()
            .padding(padding),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        item {
            Text(
                "录制记录",
                style = MaterialTheme.typography.headlineSmall,
                fontWeight = FontWeight.SemiBold,
            )
        }

        when {
            loading -> item {
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(vertical = 48.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    CircularProgressIndicator()
                }
            }
            loadError != null -> item {
                EmptyHistoryMessage(loadError.orEmpty())
            }
            recordings.isEmpty() -> item {
                EmptyHistoryMessage("还没有录制记录\n视频会保存到 Movies/ShiPing")
            }
            else -> items(recordings, key = { it.uri.toString() }) { recording ->
                RecordingListItem(
                    recording = recording,
                    playable = canOpenRecording(recording),
                    onClick = { onOpenRecording(recording) },
                    onRename = { renameTarget = recording },
                    onShowDetails = { detailsTarget = recording },
                    onDelete = { deleteTarget = recording },
                )
            }
        }
    }

    renameTarget?.let { recording ->
        RenameRecordingDialog(
            recording = recording,
            onDismiss = { renameTarget = null },
            onConfirm = { newName ->
                renameTarget = null
                coroutineScope.launch {
                    try {
                        onRenameRecording(recording, newName)
                        reloadVersion++
                    } catch (error: Exception) {
                        operationError = error.readableMessage("无法重命名录像")
                    }
                }
            },
        )
    }

    deleteTarget?.let { recording ->
        AlertDialog(
            onDismissRequest = { deleteTarget = null },
            title = { Text("删除录像？") },
            text = { Text("“${recording.displayName.removeMp4Suffix()}”将被永久删除。") },
            confirmButton = {
                TextButton(
                    onClick = {
                        deleteTarget = null
                        coroutineScope.launch {
                            try {
                                onDeleteRecording(recording)
                                reloadVersion++
                            } catch (error: Exception) {
                                operationError = error.readableMessage("无法删除录像")
                            }
                        }
                    },
                ) {
                    Text("删除", color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = {
                TextButton(onClick = { deleteTarget = null }) {
                    Text("取消")
                }
            },
        )
    }

    detailsTarget?.let { recording ->
        ModalBottomSheet(
            onDismissRequest = { detailsTarget = null },
        ) {
            RecordingDetailsSheet(
                recording = recording,
                details = recordingDetails,
                loading = detailsLoading,
                onClose = { detailsTarget = null },
            )
        }
    }

    operationError?.let { message ->
        AlertDialog(
            onDismissRequest = { operationError = null },
            title = { Text("操作失败") },
            text = { Text(message) },
            confirmButton = {
                TextButton(onClick = { operationError = null }) {
                    Text("关闭")
                }
            },
        )
    }
}

@Composable
private fun RecordingListItem(
    recording: RecordingItem,
    playable: Boolean,
    onClick: () -> Unit,
    onRename: () -> Unit,
    onShowDetails: () -> Unit,
    onDelete: () -> Unit,
) {
    val context = androidx.compose.ui.platform.LocalContext.current
    var menuExpanded by remember(recording.uri) { mutableStateOf(false) }
    val details = buildList {
        if (recording.durationMs > 0) {
            add(formatDuration(recording.durationMs))
        }
        if (recording.sizeBytes > 0) {
            add(Formatter.formatShortFileSize(context, recording.sizeBytes))
        }
        if (recording.createdAtMs > 0) {
            add(
                DateFormat.getDateTimeInstance(DateFormat.SHORT, DateFormat.SHORT)
                    .format(Date(recording.createdAtMs)),
            )
        }
    }.joinToString(" · ")

    Box(Modifier.fillMaxWidth()) {
        Card(
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(18.dp),
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.58f),
            ),
        ) {
            ListItem(
                headlineContent = {
                    Text(
                        recording.displayName.removeMp4Suffix(),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                },
                supportingContent = if (details.isNotEmpty()) {
                    { Text(details, maxLines = 1, overflow = TextOverflow.Ellipsis) }
                } else {
                    null
                },
                leadingContent = {
                    Icon(
                        painter = painterResource(R.drawable.ic_video),
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.primary,
                    )
                },
                trailingContent = if (playable) {
                    {
                        Icon(
                            painter = painterResource(R.drawable.ic_play),
                            contentDescription = "播放",
                            tint = MaterialTheme.colorScheme.primary,
                        )
                    }
                } else {
                    null
                },
                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                modifier = Modifier.combinedClickable(
                    onClickLabel = if (playable) "播放录像" else null,
                    onLongClickLabel = "录像操作",
                    onClick = { if (playable) onClick() },
                    onLongClick = { menuExpanded = true },
                ),
            )
        }

        DropdownMenu(
            expanded = menuExpanded,
            onDismissRequest = { menuExpanded = false },
        ) {
            DropdownMenuItem(
                text = { Text("重命名") },
                onClick = {
                    menuExpanded = false
                    onRename()
                },
            )
            DropdownMenuItem(
                text = { Text("详情") },
                onClick = {
                    menuExpanded = false
                    onShowDetails()
                },
            )
            HorizontalDivider()
            DropdownMenuItem(
                text = { Text("删除", color = MaterialTheme.colorScheme.error) },
                onClick = {
                    menuExpanded = false
                    onDelete()
                },
            )
        }
    }
}

@Composable
private fun RenameRecordingDialog(
    recording: RecordingItem,
    onDismiss: () -> Unit,
    onConfirm: (String) -> Unit,
) {
    var name by remember(recording.uri) {
        mutableStateOf(recording.displayName.removeMp4Suffix())
    }
    var validationError by remember(recording.uri) { mutableStateOf<String?>(null) }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("重命名录像") },
        text = {
            Column {
                OutlinedTextField(
                    value = name,
                    onValueChange = {
                        name = it
                        validationError = null
                    },
                    label = { Text("录像名称") },
                    supportingText = if (validationError != null) {
                        {
                            Text(
                                checkNotNull(validationError),
                                color = MaterialTheme.colorScheme.error,
                            )
                        }
                    } else {
                        null
                    },
                    singleLine = true,
                )
            }
        },
        confirmButton = {
            TextButton(
                onClick = {
                    val candidate = name.trim()
                    validationError = when {
                        candidate.isEmpty() -> "录像名称不能为空"
                        candidate.any { it in INVALID_FILE_NAME_CHARACTERS } ->
                            "不能包含 \\ / : * ? \" < > |"
                        else -> null
                    }
                    if (validationError == null) {
                        onConfirm(candidate)
                    }
                },
            ) {
                Text("保存")
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) {
                Text("取消")
            }
        },
    )
}

@Composable
private fun RecordingDetailsSheet(
    recording: RecordingItem,
    details: RecordingDetails?,
    loading: Boolean,
    onClose: () -> Unit,
) {
    val context = androidx.compose.ui.platform.LocalContext.current
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 24.dp, vertical = 8.dp),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                recording.displayName.removeMp4Suffix(),
                modifier = Modifier.weight(1f),
                style = MaterialTheme.typography.titleLarge,
                fontWeight = FontWeight.SemiBold,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            TextButton(onClick = onClose) {
                Text("关闭")
            }
        }
        Spacer(Modifier.height(12.dp))

        if (loading || details == null) {
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(vertical = 48.dp),
                contentAlignment = Alignment.Center,
            ) {
                CircularProgressIndicator()
            }
        } else {
            DetailRow("位置", details.location)
            DetailRow("大小", Formatter.formatFileSize(context, details.sizeBytes))
            DetailRow("创建时间", formatDateTime(details.createdAtMs))
            DetailRow("视频时长", formatElapsed(details.durationMs))
            DetailRow(
                "分辨率",
                if (details.width > 0 && details.height > 0) {
                    "${details.width} × ${details.height}"
                } else {
                    "不可用"
                },
            )
            DetailRow("总帧数", details.totalFrames.toString())
            DetailRow(
                "平均帧率",
                details.averageFrameRate?.let {
                    String.format(Locale.ROOT, "%.2f FPS", it)
                } ?: "不可用",
            )
            DetailRow("封装格式", details.containerFormat)
            DetailRow("视频格式", details.videoFormat)
            DetailRow("音频格式", details.audioFormat)
        }
        Spacer(Modifier.height(24.dp))
    }
}

@Composable
private fun DetailRow(label: String, value: String) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Text(
            label,
            modifier = Modifier.width(80.dp),
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(value, modifier = Modifier.weight(1f))
    }
}

@Composable
private fun EmptyHistoryMessage(message: String) {
    Text(
        message,
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 48.dp),
        textAlign = TextAlign.Center,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

@Composable
private fun AboutScreen(padding: PaddingValues) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(padding)
            .padding(24.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(
            painter = painterResource(R.drawable.ic_nav_record),
            contentDescription = null,
            modifier = Modifier.size(48.dp),
            tint = MaterialTheme.colorScheme.primary,
        )
        Spacer(Modifier.height(12.dp))
        Text("ShiPing", style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold)
        Text(
            "v${BuildConfig.VERSION_NAME}",
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(20.dp))
        Text(
            "轻量、直接的屏幕录制工具\n\nKotlin + Material 3 移动界面\nAndroid 原生录屏管线\n\nGitHub: dripai/shitu",
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

private fun formatDuration(durationMs: Long): String {
    val totalSeconds = durationMs / 1_000
    val minutes = totalSeconds / 60
    val seconds = totalSeconds % 60
    return String.format(Locale.ROOT, "%02d:%02d", minutes, seconds)
}

private fun formatDateTime(timestampMs: Long): String =
    if (timestampMs > 0) {
        DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.MEDIUM)
            .format(Date(timestampMs))
    } else {
        "不可用"
    }

private fun String.removeMp4Suffix(): String =
    if (endsWith(".mp4", ignoreCase = true)) dropLast(4) else this

private fun Throwable.readableMessage(defaultMessage: String): String =
    message?.takeIf(String::isNotBlank) ?: defaultMessage

private const val INVALID_FILE_NAME_CHARACTERS = "\\/:*?\"<>|"
