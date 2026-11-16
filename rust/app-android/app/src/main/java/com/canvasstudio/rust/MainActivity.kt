package com.canvasstudio.rust

import android.app.Activity
import android.app.AlertDialog
import android.content.Context
import android.graphics.Bitmap
import android.graphics.PixelFormat
import android.graphics.Typeface
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.text.InputType
import android.util.TypedValue
import android.view.Gravity
import android.view.HapticFeedbackConstants
import android.view.SurfaceHolder
import android.view.View
import android.view.ViewGroup
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.widget.BaseAdapter
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.GridView
import android.widget.HorizontalScrollView
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.PopupMenu
import android.widget.PopupWindow
import android.widget.TextView
import android.widget.Toast
import org.json.JSONArray
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicInteger

class BoardInfo(val id: String, val title: String, val updated: String, val shapes: Int)

class MainActivity : Activity(), Native.Listener, SurfaceHolder.Callback {
    private val main = Handler(Looper.getMainLooper())
    private val disk = Executors.newSingleThreadExecutor()
    private val thumbs = Executors.newSingleThreadExecutor()
    private val events = AtomicInteger(0)
    private val handwriting = Handwriting()
    private val updater by lazy { Updater(this) }
    private val prefs by lazy { getSharedPreferences("settings", Context.MODE_PRIVATE) }
    private val dir by lazy { File(filesDir, "boards").apply { mkdirs() }.absolutePath }

    private var handle = 0L
    private var boardId: String? = null
    private var boardTitle = ""
    private var dirty = false
    private var state = DoubleArray(19)
    private var modelNoticeShown = false

    private lateinit var library: LinearLayout
    private lateinit var grid: GridView
    private lateinit var empty: TextView
    private lateinit var board: FrameLayout
    private lateinit var surface: BoardView
    private lateinit var titleView: TextView
    private lateinit var styleButton: IconButton
    private lateinit var undoButton: IconButton
    private lateinit var redoButton: IconButton
    private lateinit var deleteButton: IconButton
    private val toolButtons = HashMap<Int, IconButton>()
    private var boards = listOf<BoardInfo>()
    private val thumbCache = HashMap<String, Bitmap>()
    private val adapter = BoardAdapter()

    // ------------------------------------------------------------ lifecycle

    override fun onCreate(saved: Bundle?) {
        super.onCreate(saved)
        handle = Native.create(this)
        val root = FrameLayout(this)
        root.setBackgroundColor(0xfff4f5f7.toInt())
        root.setOnApplyWindowInsetsListener { v, insets ->
            @Suppress("DEPRECATION")
            v.setPadding(insets.systemWindowInsetLeft, insets.systemWindowInsetTop, insets.systemWindowInsetRight, insets.systemWindowInsetBottom)
            insets
        }
        buildLibrary(root)
        buildBoard(root)
        setContentView(root)
        applySettings()
        if (prefs.getBoolean("text", true)) handwriting.prepare()
        updater.check(false)
        val last = saved?.getString("board")
        if (last != null) openBoard(last) else showLibrary()
    }

    override fun onSaveInstanceState(out: Bundle) {
        super.onSaveInstanceState(out)
        boardId?.let { out.putString("board", it) }
    }

    override fun onNewIntent(intent: android.content.Intent) {
        super.onNewIntent(intent)
        if (intent.action == Updater.ACTION) updater.onResult(intent)
    }

    override fun onPause() {
        super.onPause()
        saveNow(true)
    }

    override fun onDestroy() {
        super.onDestroy()
        main.removeCallbacksAndMessages(null)
        val h = handle
        handle = 0
        surface.handle = 0
        handwriting.close()
        thumbs.shutdownNow()
        // Let a save that is already queued finish before the engine goes away.
        disk.execute { Native.destroy(h) }
        disk.shutdown()
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        if (boardId != null) closeBoard() else super.onBackPressed()
    }

    private fun applySettings() {
        Native.command(handle, 13, if (prefs.getBoolean("auto", true)) 1.0 else 0.0, 0.0)
        Native.command(handle, 17, if (prefs.getBoolean("text", true)) 1.0 else 0.0, 0.0)
        Native.command(handle, 16, if (prefs.getBoolean("palm", true)) 1.0 else 0.0, 0.0)
        Native.command(handle, 10, if (prefs.getBoolean("grid", true)) 1.0 else 0.0, 0.0)
        Native.command(handle, 14, prefs.getInt("font", 0).toDouble(), 0.0)
        Native.command(handle, 1, 2.0, 0.0)
    }

    private fun now(): String = SimpleDateFormat("yyyy-MM-dd'T'HH:mm:ss.SSS", Locale.US).format(Date())

    private fun toast(text: String) = Toast.makeText(this, text, Toast.LENGTH_SHORT).show()

    // -------------------------------------------------------------- library

    private fun buildLibrary(root: FrameLayout) {
        library = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        val header = LinearLayout(this).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(24f), dp(18f), dp(24f), dp(10f))
        }
        header.addView(TextView(this).apply {
            text = "Canvas Studio"
            setTextColor(INK)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 24f)
            typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
        }, LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f))
        header.addView(TextView(this).apply {
            text = "New board"
            setTextColor(0xffffffff.toInt())
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 15f)
            typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
            background = rounded(ACCENT, dp(22f).toFloat())
            setPadding(dp(20f), dp(11f), dp(20f), dp(11f))
            isClickable = true
            isFocusable = true
            setOnClickListener { newBoard() }
        })
        library.addView(header)
        grid = GridView(this).apply {
            columnWidth = dp(230f)
            numColumns = GridView.AUTO_FIT
            stretchMode = GridView.STRETCH_COLUMN_WIDTH
            horizontalSpacing = dp(16f)
            verticalSpacing = dp(16f)
            setPadding(dp(24f), dp(10f), dp(24f), dp(24f))
            clipToPadding = false
            selector = rounded(0x00000000, 0f)
            adapter = this@MainActivity.adapter
            setOnItemClickListener { _, _, i, _ -> openBoard(boards[i].id) }
            setOnItemLongClickListener { v, _, i, _ ->
                boardMenu(v, boards[i])
                true
            }
        }
        empty = TextView(this).apply {
            text = "No boards yet. Tap New board to start drawing."
            setTextColor(0xff6b7280.toInt())
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 16f)
            gravity = Gravity.CENTER
        }
        val body = FrameLayout(this)
        body.addView(grid, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
        body.addView(empty, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
        library.addView(body, LinearLayout.LayoutParams(MATCH_PARENT, 0, 1f))
        root.addView(library, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
    }

    private fun showLibrary() {
        board.visibility = View.GONE
        library.visibility = View.VISIBLE
        refreshBoards()
    }

    private fun refreshBoards() {
        disk.execute {
            val list = ArrayList<BoardInfo>()
            try {
                val json = JSONArray(Native.listBoards(dir) ?: "[]")
                for (i in 0 until json.length()) {
                    val o = json.getJSONObject(i)
                    list.add(BoardInfo(o.getString("id"), o.optString("title", "Untitled"), o.optString("updated"), o.optInt("shapes")))
                }
            } catch (e: Exception) {
                android.util.Log.w(Handwriting.TAG, "could not list boards", e)
            }
            main.post {
                boards = list
                empty.visibility = if (list.isEmpty()) View.VISIBLE else View.GONE
                adapter.notifyDataSetChanged()
            }
        }
    }

    private fun newBoard() {
        val id = "b" + java.lang.Long.toString(System.currentTimeMillis(), 36) + Integer.toString((Math.random() * 46656).toInt(), 36)
        val title = "Board " + SimpleDateFormat("d MMM, HH:mm", Locale.getDefault()).format(Date())
        disk.execute {
            val failed = Native.createBoard(dir, id, title, now()) != 0
            main.post { if (failed) toast("Could not create the board") else openBoard(id) }
        }
    }

    private fun boardMenu(anchor: View, info: BoardInfo) {
        val menu = PopupMenu(this, anchor)
        menu.menu.add(0, 1, 0, "Rename")
        menu.menu.add(0, 2, 1, "Delete")
        menu.setOnMenuItemClickListener {
            if (it.itemId == 1) {
                askText("Rename board", info.title, false) { name ->
                    disk.execute {
                        Native.renameBoard(dir, info.id, name, now())
                        refreshBoards()
                    }
                }
            } else {
                AlertDialog.Builder(this)
                    .setMessage("Delete \"${info.title}\"? This cannot be undone.")
                    .setPositiveButton("Delete") { _, _ ->
                        disk.execute {
                            Native.deleteBoard(dir, info.id)
                            refreshBoards()
                        }
                    }
                    .setNegativeButton("Cancel", null)
                    .show()
            }
            true
        }
        menu.show()
    }

    private inner class BoardAdapter : BaseAdapter() {
        override fun getCount() = boards.size
        override fun getItem(i: Int) = boards[i]
        override fun getItemId(i: Int) = i.toLong()

        override fun getView(i: Int, old: View?, parent: ViewGroup): View {
            val card = (old as? LinearLayout) ?: LinearLayout(this@MainActivity).apply {
                orientation = LinearLayout.VERTICAL
                background = rounded(0xffffffff.toInt(), dp(12f).toFloat(), 0xffe2e5ea.toInt(), dp(1f))
                setPadding(dp(1f), dp(1f), dp(1f), dp(10f))
                addView(ImageView(context).apply { scaleType = ImageView.ScaleType.CENTER_CROP }, LinearLayout.LayoutParams(MATCH_PARENT, dp(140f)))
                addView(TextView(context).apply {
                    setTextColor(INK)
                    setTextSize(TypedValue.COMPLEX_UNIT_SP, 15f)
                    maxLines = 1
                    setPadding(dp(12f), dp(10f), dp(12f), 0)
                })
                addView(TextView(context).apply {
                    setTextColor(0xff6b7280.toInt())
                    setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
                    setPadding(dp(12f), dp(2f), dp(12f), 0)
                })
            }
            val info = boards[i]
            val image = card.getChildAt(0) as ImageView
            (card.getChildAt(1) as TextView).text = info.title
            (card.getChildAt(2) as TextView).text = if (info.shapes == 1) "1 item" else "${info.shapes} items"
            val key = info.id + "@" + info.updated
            image.tag = key
            val cached = thumbCache[key]
            image.setImageBitmap(cached)
            if (cached == null) {
                thumbs.execute {
                    val w = 460
                    val h = 280
                    val px = Native.thumbnail(dir, info.id, w, h) ?: return@execute
                    val bitmap = Bitmap.createBitmap(px, w, h, Bitmap.Config.ARGB_8888)
                    main.post {
                        if (thumbCache.size > 60) thumbCache.clear()
                        thumbCache[key] = bitmap
                        if (image.tag == key) image.setImageBitmap(bitmap)
                    }
                }
            }
            return card
        }
    }

    // ---------------------------------------------------------------- board

    private fun buildBoard(root: FrameLayout) {
        board = FrameLayout(this)
        surface = BoardView(this)
        surface.handle = handle
        surface.holder.setFormat(PixelFormat.RGBX_8888)
        surface.holder.addCallback(this)
        board.addView(surface, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))

        val panel = { rounded(0xffffffff.toInt(), dp(14f).toFloat(), 0xffdfe2e7.toInt(), dp(1f)) }

        // Top left: back to the boards, and the board's name.
        val top = LinearLayout(this).apply {
            gravity = Gravity.CENTER_VERTICAL
            background = panel()
            elevation = dp(3f).toFloat()
            setPadding(dp(2f), 0, dp(14f), 0)
        }
        top.addView(IconButton(this, Icon.BACK, "Boards").apply { setOnClickListener { closeBoard() } })
        titleView = TextView(this).apply {
            setTextColor(INK)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 15f)
            maxLines = 1
            maxWidth = dp(260f)
            isClickable = true
            setOnClickListener { renameOpenBoard() }
        }
        top.addView(titleView)
        board.addView(top, FrameLayout.LayoutParams(WRAP_CONTENT, WRAP_CONTENT, Gravity.TOP or Gravity.START).apply {
            setMargins(dp(12f), dp(12f), 0, 0)
        })

        // Bottom centre: tools. There is no text button: writing turns into text by itself.
        val bar = LinearLayout(this).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(4f), dp(2f), dp(4f), dp(2f))
        }
        fun tool(index: Int, icon: Icon, name: String) {
            val b = IconButton(this, icon, name)
            b.setOnClickListener { Native.command(handle, 1, index.toDouble(), 0.0) }
            toolButtons[index] = b
            bar.addView(b)
        }
        fun divider() = bar.addView(View(this).apply { setBackgroundColor(0xffe2e5ea.toInt()) },
            LinearLayout.LayoutParams(dp(1f), dp(26f)).apply { setMargins(dp(5f), 0, dp(5f), 0) })
        tool(0, Icon.SELECT, "Select")
        tool(1, Icon.HAND, "Move the board")
        tool(2, Icon.PEN, "Pen")
        tool(7, Icon.ERASER, "Eraser")
        tool(3, Icon.RECTANGLE, "Rectangle")
        tool(4, Icon.ELLIPSE, "Ellipse")
        tool(5, Icon.ARROW, "Arrow")
        divider()
        styleButton = IconButton(this, Icon.STYLE, "Colour and thickness").apply { setOnClickListener { stylePopup(it) } }
        bar.addView(styleButton)
        bar.addView(IconButton(this, Icon.FONT, "Font").apply { setOnClickListener { fontMenu(it) } })
        divider()
        undoButton = IconButton(this, Icon.UNDO, "Undo").apply { setOnClickListener { Native.command(handle, 5, 0.0, 0.0) } }
        redoButton = IconButton(this, Icon.REDO, "Redo").apply { setOnClickListener { Native.command(handle, 6, 0.0, 0.0) } }
        deleteButton = IconButton(this, Icon.DELETE, "Delete selection").apply { setOnClickListener { Native.command(handle, 7, 0.0, 0.0) } }
        bar.addView(undoButton)
        bar.addView(redoButton)
        bar.addView(deleteButton)
        bar.addView(IconButton(this, Icon.MORE, "More").apply { setOnClickListener { moreMenu(it) } })
        val scroll = HorizontalScrollView(this).apply {
            isHorizontalScrollBarEnabled = false
            background = panel()
            elevation = dp(3f).toFloat()
            addView(bar)
        }
        board.addView(scroll, FrameLayout.LayoutParams(WRAP_CONTENT, WRAP_CONTENT, Gravity.BOTTOM or Gravity.CENTER_HORIZONTAL).apply {
            setMargins(dp(12f), 0, dp(12f), dp(14f))
        })
        board.visibility = View.GONE
        root.addView(board, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
    }

    private fun openBoard(id: String) {
        disk.execute {
            val title = Native.loadBoard(handle, dir, id)
            main.post {
                if (title == null) {
                    toast("This board could not be opened")
                    showLibrary()
                    return@post
                }
                boardId = id
                boardTitle = title
                dirty = false
                titleView.text = title
                library.visibility = View.GONE
                board.visibility = View.VISIBLE
                refreshState()
            }
        }
    }

    private fun closeBoard() {
        saveNow(true)
        boardId = null
        showLibrary()
    }

    private fun renameOpenBoard() {
        askText("Rename board", boardTitle, false) { name ->
            boardTitle = name
            titleView.text = name
            dirty = true
            saveNow(false)
        }
    }

    private val saveLater = Runnable { saveNow(false) }

    /** Writes the open board if it changed. `force` saves even mid-stroke. */
    private fun saveNow(force: Boolean) {
        main.removeCallbacks(saveLater)
        val id = boardId ?: return
        if (!dirty || handle == 0L) return
        dirty = false
        val (h, title, stamp) = Triple(handle, boardTitle, now())
        disk.execute {
            val result = Native.saveBoard(h, dir, id, title, stamp, if (force) 1 else 0)
            if (result != 0) {
                main.post {
                    if (boardId == id) {
                        dirty = true
                        main.postDelayed(saveLater, 700)
                    }
                    if (result == 1) toast("Could not save the board")
                }
            }
        }
    }

    // ------------------------------------------------------- engine events

    /** Render thread. */
    override fun onEvent(bits: Int) {
        if (events.getAndAccumulate(bits) { a, b -> a or b } == 0) main.post(drain)
    }

    private val drain = Runnable {
        val bits = events.getAndSet(0)
        if (handle == 0L) return@Runnable
        if (bits and (Native.EV_STATE or Native.EV_DOC or Native.EV_VIEW) != 0) refreshState()
        if (bits and Native.EV_DOC != 0 && boardId != null) {
            dirty = true
            main.removeCallbacks(saveLater)
            main.postDelayed(saveLater, 1200)
        }
        if (bits and Native.EV_HAPTIC != 0) surface.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
        if (bits and Native.EV_TEXT != 0) editText()
        if (bits and Native.EV_INK != 0) readInk()
    }

    private fun refreshState() {
        val s = Native.state(handle) ?: return
        state = s
        for ((index, button) in toolButtons) button.active = s[0].toInt() == index
        undoButton.isEnabled = s[2] != 0.0
        redoButton.isEnabled = s[3] != 0.0
        deleteButton.visibility = if (s[4] > 0) View.VISIBLE else View.GONE
        styleButton.swatch = s[6].toLong().toInt()
    }

    private fun readInk() {
        while (true) {
            val request = Native.takeInkRequest(handle) ?: break
            if (request.size < 2) continue
            val token = request[0].toInt()
            if (!handwriting.ready && !modelNoticeShown) {
                modelNoticeShown = true
                toast("Getting the handwriting reader ready. Writing will turn into text once it has downloaded.")
            }
            handwriting.read(request) { text -> if (handle != 0L) Native.commitInk(handle, token, text) }
        }
    }

    /** The engine asks for this when a piece of text is double tapped. */
    private fun editText() {
        val request = Native.takeTextRequest(handle) ?: return
        val parts = request.split('\u001f', limit = 4)
        if (parts.size < 4) return
        val x = parts[1].toDoubleOrNull() ?: return
        val y = parts[2].toDoubleOrNull() ?: return
        askText(if (parts[0].isEmpty()) "Add text" else "Edit text", parts[3], true) { text ->
            Native.commitText(handle, parts[0], x, y, text)
        }
    }

    private fun askText(title: String, initial: String, multiline: Boolean, done: (String) -> Unit) {
        val input = EditText(this).apply {
            setText(initial)
            setSelection(initial.length)
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES or
                (if (multiline) InputType.TYPE_TEXT_FLAG_MULTI_LINE else 0)
        }
        val frame = FrameLayout(this).apply {
            setPadding(dp(20f), dp(8f), dp(20f), 0)
            addView(input)
        }
        val dialog = AlertDialog.Builder(this)
            .setTitle(title)
            .setView(frame)
            .setPositiveButton("Done") { _, _ ->
                val text = input.text.toString().trim()
                if (text.isNotEmpty()) done(text)
            }
            .setNegativeButton("Cancel", null)
            .create()
        dialog.window?.setSoftInputMode(android.view.WindowManager.LayoutParams.SOFT_INPUT_STATE_VISIBLE)
        dialog.show()
        input.requestFocus()
    }

    // ---------------------------------------------------------------- menus

    private fun stylePopup(anchor: View) {
        val box = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            background = rounded(0xffffffff.toInt(), dp(14f).toFloat(), 0xffdfe2e7.toInt(), dp(1f))
            setPadding(dp(10f), dp(10f), dp(10f), dp(10f))
        }
        val popup = PopupWindow(box, WRAP_CONTENT, WRAP_CONTENT, true)
        popup.elevation = dp(8f).toFloat()
        val current = state[6].toLong().toInt()
        val colours = LinearLayout(this)
        for (c in COLOURS) {
            colours.addView(View(this).apply {
                background = rounded(c, dp(15f).toFloat(), if (c == current) ACCENT else 0x33000000, dp(if (c == current) 3f else 1f))
                contentDescription = "Colour"
                setOnClickListener {
                    Native.command(handle, 2, (c.toLong() and 0xffffffffL).toDouble(), 0.0)
                    popup.dismiss()
                }
            }, LinearLayout.LayoutParams(dp(30f), dp(30f)).apply { setMargins(dp(5f), dp(5f), dp(5f), dp(5f)) })
        }
        box.addView(colours)
        val widths = LinearLayout(this).apply { gravity = Gravity.CENTER_VERTICAL }
        for (w in WIDTHS) {
            val chosen = Math.abs(state[7] - w) < 0.01
            val cell = FrameLayout(this).apply {
                background = rounded(if (chosen) 0x1f2f6fed else 0x00000000, dp(8f).toFloat())
                contentDescription = "Thickness"
                setOnClickListener {
                    Native.command(handle, 3, w, 0.0)
                    popup.dismiss()
                }
            }
            val dot = dp(w.toFloat() * 1.6f + 3f)
            cell.addView(View(this).apply { background = rounded(INK, dot / 2f) }, FrameLayout.LayoutParams(dot, dot, Gravity.CENTER))
            widths.addView(cell, LinearLayout.LayoutParams(dp(40f), dp(40f)))
        }
        val filled = state[8] != 0.0
        widths.addView(TextView(this).apply {
            text = if (filled) "Filled" else "Outline"
            setTextColor(if (filled) ACCENT else INK)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 14f)
            gravity = Gravity.CENTER
            setPadding(dp(14f), 0, dp(10f), 0)
            setOnClickListener {
                Native.command(handle, 4, if (filled) 0.0 else 1.0, 0.0)
                popup.dismiss()
            }
        }, LinearLayout.LayoutParams(WRAP_CONTENT, dp(40f)))
        box.addView(widths)
        box.measure(View.MeasureSpec.UNSPECIFIED, View.MeasureSpec.UNSPECIFIED)
        popup.showAsDropDown(anchor, -box.measuredWidth / 2 + anchor.width / 2, -anchor.height - box.measuredHeight - dp(10f))
    }

    private fun fontMenu(anchor: View) {
        val menu = PopupMenu(this, anchor, Gravity.TOP)
        val current = state[14].toInt()
        for ((i, name) in TextHost.FONT_NAMES.withIndex()) {
            menu.menu.add(1, i, i, name).isChecked = i == current
        }
        menu.menu.setGroupCheckable(1, true, true)
        if (state[16] > 0) {
            menu.menu.add(2, 100, 10, "Larger text")
            menu.menu.add(2, 101, 11, "Smaller text")
        }
        menu.setOnMenuItemClickListener {
            when (it.itemId) {
                100 -> Native.command(handle, 15, 1.25, 0.0)
                101 -> Native.command(handle, 15, 0.8, 0.0)
                else -> {
                    prefs.edit().putInt("font", it.itemId).apply()
                    Native.command(handle, 14, it.itemId.toDouble(), 0.0)
                }
            }
            true
        }
        menu.show()
    }

    private fun moreMenu(anchor: View) {
        val menu = PopupMenu(this, anchor, Gravity.TOP)
        fun toggle(id: Int, title: String, on: Boolean) = menu.menu.add(0, id, id, title).apply {
            isCheckable = true
            isChecked = on
        }
        toggle(1, "Tidy drawn shapes", state[15] != 0.0)
        toggle(9, "Turn handwriting into typed text", state.size > 18 && state[18] != 0.0)
        toggle(2, "Ignore palm touches", state[17] != 0.0)
        toggle(3, "Grid", state[11] != 0.0)
        menu.menu.add(0, 4, 4, "Show everything")
        menu.menu.add(0, 5, 5, "Zoom to 100% (now ${Math.round(state[5] * 100)}%)")
        menu.menu.add(0, 6, 6, "Zoom in")
        menu.menu.add(0, 7, 7, "Zoom out")
        menu.menu.add(0, 8, 8, "Check for updates")
        menu.setOnMenuItemClickListener {
            val on = !it.isChecked
            when (it.itemId) {
                1 -> {
                    prefs.edit().putBoolean("auto", on).apply()
                    Native.command(handle, 13, if (on) 1.0 else 0.0, 0.0)
                }
                9 -> {
                    prefs.edit().putBoolean("text", on).apply()
                    Native.command(handle, 17, if (on) 1.0 else 0.0, 0.0)
                    if (on) handwriting.prepare()
                }
                2 -> {
                    prefs.edit().putBoolean("palm", on).apply()
                    Native.command(handle, 16, if (on) 1.0 else 0.0, 0.0)
                }
                3 -> {
                    prefs.edit().putBoolean("grid", on).apply()
                    Native.command(handle, 10, if (on) 1.0 else 0.0, 0.0)
                }
                4 -> Native.command(handle, 8, 0.0, 0.0)
                5 -> Native.command(handle, 12, 0.0, 0.0)
                6 -> Native.command(handle, 9, 1.25, 0.0)
                7 -> Native.command(handle, 9, 0.8, 0.0)
                8 -> updater.check(true)
            }
            // Toggles do not always raise a state event, so read the state back.
            main.postDelayed({ if (handle != 0L) refreshState() }, 60)
            true
        }
        menu.show()
    }

    // -------------------------------------------------------------- surface

    override fun surfaceCreated(holder: SurfaceHolder) {}

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        if (handle != 0L) Native.surface(handle, holder.surface, width, height, resources.displayMetrics.density.toDouble())
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        if (handle != 0L) Native.surface(handle, null, 0, 0, resources.displayMetrics.density.toDouble())
    }

    companion object {
        val COLOURS = intArrayOf(
            0xff1d1d1d.toInt(), 0xff6b7280.toInt(), 0xffe03131.toInt(), 0xfff08c00.toInt(),
            0xff2f9e44.toInt(), 0xff2f6fed.toInt(), 0xff9c36b5.toInt(), 0xffffffff.toInt(),
        )
        val WIDTHS = doubleArrayOf(2.0, 3.5, 6.0, 10.0)
    }
}
