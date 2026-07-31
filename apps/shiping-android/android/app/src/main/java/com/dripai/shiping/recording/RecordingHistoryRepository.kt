package com.dripai.shiping.recording

import android.content.ContentUris
import android.content.ContentValues
import android.content.Context
import android.media.MediaExtractor
import android.media.MediaFormat
import android.media.MediaMetadataRetriever
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import androidx.core.content.FileProvider
import com.dripai.shiping.BuildConfig
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File

data class RecordingItem(
    val uri: Uri,
    val displayName: String,
    val durationMs: Long,
    val sizeBytes: Long,
    val createdAtMs: Long,
    val storageLocation: String,
    val legacyFile: File? = null,
)

data class RecordingDetails(
    val location: String,
    val sizeBytes: Long,
    val createdAtMs: Long,
    val durationMs: Long,
    val width: Int,
    val height: Int,
    val totalFrames: Long,
    val averageFrameRate: Double?,
    val containerFormat: String,
    val videoFormat: String,
    val audioFormat: String,
)

class RecordingHistoryRepository(
    private val context: Context,
) {
    suspend fun load(): List<RecordingItem> = withContext(Dispatchers.IO) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            loadFromMediaStore()
        } else {
            loadFromLegacyDirectory()
        }
    }

    suspend fun rename(recording: RecordingItem, requestedName: String): RecordingItem =
        withContext(Dispatchers.IO) {
            val displayName = normalizedDisplayName(requestedName)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                val values = ContentValues().apply {
                    put(MediaStore.Video.Media.DISPLAY_NAME, displayName)
                }
                check(context.contentResolver.update(recording.uri, values, null, null) == 1) {
                    "系统没有更新该录像名称"
                }
                recording.copy(
                    displayName = displayName,
                    storageLocation = "$MEDIA_DIRECTORY$displayName",
                )
            } else {
                val source = checkNotNull(recording.legacyFile) { "缺少录像文件路径" }
                val destination = File(source.parentFile, displayName)
                check(!destination.exists()) { "同名录像已经存在" }
                check(source.renameTo(destination)) { "无法重命名录像文件" }
                recording.copy(
                    uri = FileProvider.getUriForFile(
                        context,
                        "${BuildConfig.APPLICATION_ID}.files",
                        destination,
                    ),
                    displayName = displayName,
                    storageLocation = destination.absolutePath,
                    legacyFile = destination,
                )
            }
        }

    suspend fun delete(recording: RecordingItem) = withContext(Dispatchers.IO) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            check(context.contentResolver.delete(recording.uri, null, null) == 1) {
                "系统没有删除该录像"
            }
        } else {
            val file = checkNotNull(recording.legacyFile) { "缺少录像文件路径" }
            check(file.delete()) { "无法删除录像文件" }
        }
    }

    suspend fun loadDetails(recording: RecordingItem): RecordingDetails =
        withContext(Dispatchers.IO) {
            readDetails(recording)
        }

    private fun loadFromMediaStore(): List<RecordingItem> {
        val collection = MediaStore.Video.Media.getContentUri(
            MediaStore.VOLUME_EXTERNAL_PRIMARY,
        )
        val projection = arrayOf(
            MediaStore.Video.Media._ID,
            MediaStore.Video.Media.DISPLAY_NAME,
            MediaStore.Video.Media.DURATION,
            MediaStore.Video.Media.SIZE,
            MediaStore.Video.Media.DATE_ADDED,
        )
        val directory = "${Environment.DIRECTORY_MOVIES}/ShiPing/"
        val recordings = mutableListOf<RecordingItem>()

        context.contentResolver.query(
            collection,
            projection,
            "${MediaStore.Video.Media.RELATIVE_PATH} = ?",
            arrayOf(directory),
            "${MediaStore.Video.Media.DATE_ADDED} DESC",
        )?.use { cursor ->
            val idColumn = cursor.getColumnIndexOrThrow(MediaStore.Video.Media._ID)
            val nameColumn = cursor.getColumnIndexOrThrow(MediaStore.Video.Media.DISPLAY_NAME)
            val durationColumn = cursor.getColumnIndexOrThrow(MediaStore.Video.Media.DURATION)
            val sizeColumn = cursor.getColumnIndexOrThrow(MediaStore.Video.Media.SIZE)
            val dateColumn = cursor.getColumnIndexOrThrow(MediaStore.Video.Media.DATE_ADDED)

            while (cursor.moveToNext()) {
                val displayName = cursor.getString(nameColumn) ?: "ShiPing 录制.mp4"
                recordings += RecordingItem(
                    uri = ContentUris.withAppendedId(collection, cursor.getLong(idColumn)),
                    displayName = displayName,
                    durationMs = cursor.getLong(durationColumn).coerceAtLeast(0),
                    sizeBytes = cursor.getLong(sizeColumn).coerceAtLeast(0),
                    createdAtMs = cursor.getLong(dateColumn).coerceAtLeast(0) * 1_000,
                    storageLocation = "$directory$displayName",
                )
            }
        }
        return recordings
    }

    private fun loadFromLegacyDirectory(): List<RecordingItem> {
        val root = context.getExternalFilesDir(Environment.DIRECTORY_MOVIES)
            ?: return emptyList()
        val directory = File(root, "ShiPing")
        val files = directory.listFiles { file ->
            file.isFile && file.extension.equals("mp4", ignoreCase = true)
        }.orEmpty()

        return files
            .sortedByDescending(File::lastModified)
            .map { file ->
                RecordingItem(
                    uri = FileProvider.getUriForFile(
                        context,
                        "${BuildConfig.APPLICATION_ID}.files",
                        file,
                    ),
                    displayName = file.name,
                    durationMs = readDuration(file),
                    sizeBytes = file.length(),
                    createdAtMs = file.lastModified(),
                    storageLocation = file.absolutePath,
                    legacyFile = file,
                )
            }
    }

    private fun readDetails(recording: RecordingItem): RecordingDetails {
        val extractor = MediaExtractor()
        try {
            extractor.setDataSource(context, recording.uri, null)
            var videoTrack = -1
            var videoFormat: MediaFormat? = null
            var audioMime: String? = null

            repeat(extractor.trackCount) { index ->
                val format = extractor.getTrackFormat(index)
                val mime = format.getString(MediaFormat.KEY_MIME).orEmpty()
                when {
                    mime.startsWith("video/") && videoTrack < 0 -> {
                        videoTrack = index
                        videoFormat = format
                    }
                    mime.startsWith("audio/") && audioMime == null -> audioMime = mime
                }
            }

            check(videoTrack >= 0 && videoFormat != null) { "录像中没有可识别的视频轨道" }
            val format = checkNotNull(videoFormat)
            extractor.selectTrack(videoTrack)

            var totalFrames = 0L
            while (extractor.sampleTrackIndex >= 0) {
                totalFrames++
                if (!extractor.advance()) {
                    break
                }
            }

            val durationUs = if (format.containsKey(MediaFormat.KEY_DURATION)) {
                format.getLong(MediaFormat.KEY_DURATION).coerceAtLeast(0)
            } else {
                recording.durationMs.coerceAtLeast(0) * 1_000
            }
            val rotation = if (format.containsKey(MediaFormat.KEY_ROTATION)) {
                format.getInteger(MediaFormat.KEY_ROTATION)
            } else {
                0
            }
            val encodedWidth = format.getInteger(MediaFormat.KEY_WIDTH).coerceAtLeast(0)
            val encodedHeight = format.getInteger(MediaFormat.KEY_HEIGHT).coerceAtLeast(0)
            val (width, height) = if (rotation == 90 || rotation == 270) {
                encodedHeight to encodedWidth
            } else {
                encodedWidth to encodedHeight
            }
            val averageFrameRate = if (durationUs > 0 && totalFrames > 0) {
                totalFrames * 1_000_000.0 / durationUs
            } else {
                null
            }

            return RecordingDetails(
                location = recording.storageLocation,
                sizeBytes = recording.sizeBytes,
                createdAtMs = recording.createdAtMs,
                durationMs = if (durationUs > 0) durationUs / 1_000 else recording.durationMs,
                width = width,
                height = height,
                totalFrames = totalFrames,
                averageFrameRate = averageFrameRate,
                containerFormat = "MP4",
                videoFormat = friendlyMediaFormat(
                    format.getString(MediaFormat.KEY_MIME).orEmpty(),
                ),
                audioFormat = audioMime?.let(::friendlyMediaFormat) ?: "无音频",
            )
        } finally {
            extractor.release()
        }
    }

    private fun normalizedDisplayName(requestedName: String): String {
        val trimmed = requestedName.trim()
        val baseName = if (trimmed.endsWith(".mp4", ignoreCase = true)) {
            trimmed.dropLast(4).trim()
        } else {
            trimmed
        }
        require(baseName.isNotEmpty()) { "录像名称不能为空" }
        require(baseName.none { it in INVALID_FILE_NAME_CHARACTERS }) {
            "录像名称不能包含 \\ / : * ? \" < > |"
        }
        return "$baseName.mp4"
    }

    private fun readDuration(file: File): Long {
        val retriever = MediaMetadataRetriever()
        return try {
            retriever.setDataSource(file.absolutePath)
            retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_DURATION)
                ?.toLongOrNull()
                ?.coerceAtLeast(0)
                ?: 0
        } catch (_: RuntimeException) {
            0
        } finally {
            retriever.release()
        }
    }

    companion object {
        private const val MEDIA_DIRECTORY = "Movies/ShiPing/"
        private const val INVALID_FILE_NAME_CHARACTERS = "\\/:*?\"<>|"

        private fun friendlyMediaFormat(mime: String): String = when (mime) {
            MediaFormat.MIMETYPE_VIDEO_AVC -> "H.264 / AVC"
            MediaFormat.MIMETYPE_AUDIO_AAC -> "AAC"
            else -> mime.ifBlank { "未知" }
        }
    }
}
