"""
What the desktop can say, and a WebDriver screenshot cannot.

The compositing model is a child HWND that mpv draws into, sitting *behind* a
transparent WebView2 (README §2.1). WebDriver photographs the WebView, so its
screenshots show the UI over nothing at all — the one question Phase 0 asks is the one
question that view cannot answer.

So ask the desktop instead. Capture the screen where the window is, and read the window
tree the compositing claim is made of: two captures a second apart with different pixels
in them is video being decoded, composited and presented, and one top-level window with
the surface as its child is video *behind the UI* rather than in a window of its own.

Windows only, standard library only — this runs on the machine under test, which has
whatever Python is already there.
"""
import ctypes
import ctypes.wintypes as w
import struct
import zlib

user32 = ctypes.WinDLL("user32", use_last_error=True)
gdi32 = ctypes.WinDLL("gdi32", use_last_error=True)

# Handles are pointers. Without these, ctypes assumes a 32-bit int return and truncates
# every HDC and HBITMAP on a 64-bit build — which shows up as a blank capture rather
# than as an error.
for _fn, _res, _args in (
    (user32.GetDC, ctypes.c_void_p, [ctypes.c_void_p]),
    (user32.ReleaseDC, ctypes.c_int, [ctypes.c_void_p, ctypes.c_void_p]),
    (user32.GetWindow, ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_uint]),
    (user32.GetParent, ctypes.c_void_p, [ctypes.c_void_p]),
    (gdi32.CreateCompatibleDC, ctypes.c_void_p, [ctypes.c_void_p]),
    (gdi32.CreateCompatibleBitmap, ctypes.c_void_p,
     [ctypes.c_void_p, ctypes.c_int, ctypes.c_int]),
    (gdi32.SelectObject, ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_void_p]),
    (gdi32.DeleteObject, ctypes.c_int, [ctypes.c_void_p]),
    (gdi32.DeleteDC, ctypes.c_int, [ctypes.c_void_p]),
    (gdi32.BitBlt, ctypes.c_bool,
     [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
      ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_uint]),
    (gdi32.GetDIBits, ctypes.c_int,
     [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint,
      ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint]),
    (user32.IsWindowVisible, ctypes.c_bool, [ctypes.c_void_p]),
    (user32.GetWindowTextW, ctypes.c_int,
     [ctypes.c_void_p, ctypes.c_wchar_p, ctypes.c_int]),
    (user32.GetClassNameW, ctypes.c_int,
     [ctypes.c_void_p, ctypes.c_wchar_p, ctypes.c_int]),
    (user32.GetWindowThreadProcessId, w.DWORD, [ctypes.c_void_p, ctypes.c_void_p]),
    (user32.GetWindowLongW, ctypes.c_long, [ctypes.c_void_p, ctypes.c_int]),
    (user32.GetClientRect, ctypes.c_bool, [ctypes.c_void_p, ctypes.c_void_p]),
    (user32.GetWindowRect, ctypes.c_bool, [ctypes.c_void_p, ctypes.c_void_p]),
    (user32.ClientToScreen, ctypes.c_bool, [ctypes.c_void_p, ctypes.c_void_p]),
    (user32.SetForegroundWindow, ctypes.c_bool, [ctypes.c_void_p]),
    (user32.GetForegroundWindow, ctypes.c_void_p, []),
    (user32.SetWindowPos, ctypes.c_bool,
     [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int,
      ctypes.c_int, ctypes.c_uint]),
):
    _fn.restype, _fn.argtypes = _res, _args

GW_CHILD, GW_HWNDNEXT = 5, 2
GWL_STYLE = -16
WS_CHILD = 0x40000000
SRCCOPY = 0x00CC0020


def top_levels(pid=None):
    """Every visible top-level window, or only one process's."""
    out = []

    @ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_void_p)
    def each(hwnd, _):
        if user32.IsWindowVisible(hwnd):
            if pid is None or pid_of(hwnd) == pid:
                out.append(hwnd)
        return True

    user32.EnumWindows(each, None)
    return out


def title(hwnd):
    buf = ctypes.create_unicode_buffer(512)
    user32.GetWindowTextW(hwnd, buf, 512)
    return buf.value


def class_name(hwnd):
    buf = ctypes.create_unicode_buffer(256)
    user32.GetClassNameW(hwnd, buf, 256)
    return buf.value


def pid_of(hwnd):
    owner = w.DWORD()
    user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
    return owner.value


def find(needle):
    """The first visible top-level window whose title contains `needle`."""
    for hwnd in top_levels():
        if needle.lower() in title(hwnd).lower():
            return hwnd
    return None


def children_front_to_back(parent):
    """Direct children in z-order, front first — which is what HWND_BOTTOM is about."""
    out = []
    child = user32.GetWindow(parent, GW_CHILD)
    while child:
        out.append(child)
        child = user32.GetWindow(child, GW_HWNDNEXT)
    return out


def describe(hwnd):
    return f"{class_name(hwnd)!r} {title(hwnd)!r} at {window_rect(hwnd)}"


def is_child_of(hwnd, parent):
    style = user32.GetWindowLongW(hwnd, GWL_STYLE)
    return bool(style & WS_CHILD) and user32.GetParent(hwnd) == parent


def window_rect(hwnd):
    r = w.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(r))
    return r.left, r.top, r.right - r.left, r.bottom - r.top


def client_rect(hwnd):
    """The client area in screen coordinates: what a capture of the window covers."""
    r = w.RECT()
    user32.GetClientRect(hwnd, ctypes.byref(r))
    origin = w.POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(origin))
    return origin.x, origin.y, r.right - r.left, r.bottom - r.top


def bring_to_front(hwnd):
    """Put the window where a screen capture can see it.

    `SetForegroundWindow` is refused to a process that does not own the foreground,
    which this one does not — so raise the window in the z-order instead, which any
    process may do, and which is all a capture needs.
    """
    HWND_TOPMOST, HWND_NOTOPMOST = -1, -2
    SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW = 0x0002, 0x0001, 0x0040
    flags = SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW
    user32.SetWindowPos(hwnd, ctypes.c_void_p(HWND_TOPMOST), 0, 0, 0, 0, flags)
    user32.SetWindowPos(hwnd, ctypes.c_void_p(HWND_NOTOPMOST), 0, 0, 0, 0, flags)
    user32.SetForegroundWindow(hwnd)


def is_foreground(hwnd):
    return user32.GetForegroundWindow() == hwnd


def move_to(hwnd, x, y):
    """Move the window without resizing it, which is how it changes monitor."""
    SWP_NOSIZE, SWP_NOZORDER = 0x0001, 0x0004
    user32.SetWindowPos(hwnd, None, int(x), int(y), 0, 0, SWP_NOSIZE | SWP_NOZORDER)


def monitors():
    """Every display, as (x, y, width, height) in virtual-screen coordinates."""
    out = []

    class MonInfo(ctypes.Structure):
        _fields_ = [("cbSize", w.DWORD), ("rcMonitor", w.RECT), ("rcWork", w.RECT),
                    ("dwFlags", w.DWORD)]

    @ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_void_p,
                        ctypes.POINTER(w.RECT), ctypes.c_void_p)
    def each(handle, _dc, _rect, _data):
        info = MonInfo()
        info.cbSize = ctypes.sizeof(MonInfo)
        user32.GetMonitorInfoW(handle, ctypes.byref(info))
        r = info.rcMonitor
        out.append((r.left, r.top, r.right - r.left, r.bottom - r.top))
        return True

    user32.EnumDisplayMonitors(None, None, each, 0)
    return out


def resize(hwnd, width, height):
    """Resize the window, which is how `WindowEvent::Resized` gets raised at all."""
    SWP_NOMOVE, SWP_NOZORDER = 0x0002, 0x0004
    user32.SetWindowPos(
        hwnd, None, 0, 0, int(width), int(height), SWP_NOMOVE | SWP_NOZORDER)


def grab(x, y, width, height):
    """The screen itself, rather than a window's own drawing.

    `PrintWindow` asks a window to repaint into a bitmap, which is exactly the wrong
    question here: the parent would paint the WebView and knows nothing of what the
    child mpv is presenting into. The desktop DC holds the composited result — what a
    person looking at the screen sees.
    """
    screen = user32.GetDC(None)
    mem = gdi32.CreateCompatibleDC(screen)
    bmp = gdi32.CreateCompatibleBitmap(screen, width, height)
    gdi32.SelectObject(mem, bmp)
    gdi32.BitBlt(mem, 0, 0, width, height, screen, x, y, SRCCOPY)

    class Header(ctypes.Structure):
        _fields_ = [
            ("biSize", w.DWORD), ("biWidth", w.LONG), ("biHeight", w.LONG),
            ("biPlanes", w.WORD), ("biBitCount", w.WORD), ("biCompression", w.DWORD),
            ("biSizeImage", w.DWORD), ("biXPelsPerMeter", w.LONG),
            ("biYPelsPerMeter", w.LONG), ("biClrUsed", w.DWORD),
            ("biClrImportant", w.DWORD),
        ]

    info = Header()
    info.biSize = ctypes.sizeof(Header)
    info.biWidth = width
    info.biHeight = -height  # top-down, so row 0 is the top of the picture
    info.biPlanes = 1
    info.biBitCount = 32
    buf = ctypes.create_string_buffer(width * height * 4)
    rows = gdi32.GetDIBits(mem, bmp, 0, height, buf, ctypes.byref(info), 0)
    gdi32.DeleteObject(bmp)
    gdi32.DeleteDC(mem)
    user32.ReleaseDC(None, screen)
    if rows != height:
        raise RuntimeError(f"captured {rows} of {height} rows")
    return Image(width, height, bytes(buf))


class Image:
    """A BGRA capture, and the two questions worth asking of it."""

    def __init__(self, width, height, px):
        self.width, self.height, self.px = width, height, px

    def pixels(self, step=8, region=None):
        """A sampled grid as (x, y, r, g, b).

        Sampled because a two-megapixel frame does not need every pixel to answer
        whether it is a picture or a flat colour. `region` is (x, y, w, h) in image
        coordinates, for asking about the part of the window the video is behind.
        """
        x0, y0, width, height = region or (0, 0, self.width, self.height)
        for y in range(y0, min(y0 + height, self.height), step):
            row = y * self.width * 4
            for x in range(x0, min(x0 + width, self.width), step):
                i = row + x * 4
                yield x, y, self.px[i + 2], self.px[i + 1], self.px[i]

    def colours(self, step=8, region=None):
        return {(r, g, b) for _, _, r, g, b in self.pixels(step, region)}

    def changed_against(self, other, step=8, region=None, tolerance=12):
        """The fraction of sampled pixels that moved. Motion, as one number."""
        mine = list(self.pixels(step, region))
        theirs = list(other.pixels(step, region))
        if len(mine) != len(theirs):
            raise ValueError("captures are different sizes")
        moved = sum(
            1
            for (_, _, r1, g1, b1), (_, _, r2, g2, b2) in zip(mine, theirs)
            if abs(r1 - r2) + abs(g1 - g2) + abs(b1 - b2) > tolerance
        )
        return moved / max(len(mine), 1)

    def save_png(self, path):
        """PNG rather than BMP, so the evidence can simply be looked at."""
        raw = bytearray()
        for y in range(self.height):
            raw.append(0)  # filter: none
            row = self.px[y * self.width * 4:(y + 1) * self.width * 4]
            for x in range(0, len(row), 4):
                raw += bytes((row[x + 2], row[x + 1], row[x], 255))

        def chunk(kind, data):
            return (
                struct.pack(">I", len(data)) + kind + data
                + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
            )

        with open(path, "wb") as f:
            f.write(
                b"\x89PNG\r\n\x1a\n"
                + chunk(b"IHDR",
                        struct.pack(">IIBBBBB", self.width, self.height, 8, 6, 0, 0, 0))
                + chunk(b"IDAT", zlib.compress(bytes(raw), 6))
                + chunk(b"IEND", b"")
            )
        return path
