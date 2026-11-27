Pikslar 0.9.0-beta.1 for Windows (64-bit)
=========================================

Pikslar is a pixel-art editor and animation studio for Windows. This is a
BETA: an early test version. It has been tested by automated checks on one
Windows 11 computer only. No person outside development has used it yet, so
please keep copies of any artwork you care about, and save beta work into a
folder of its own.

Free for noncommercial use and sharing: see LICENSE.txt.


What you need
-------------

- Windows 11, or Windows 10 version 1703 or later, 64-bit (x64).
  Windows 10 has not been tested at all; see KNOWN-ISSUES.txt. Older Windows
  (Windows 10 before 1703, 8.1, 7) cannot start this program.
- No installation, no administrator rights and no internet connection.


What is in this folder
----------------------

  pikslar.exe              the editor
  pikslar-cli.exe          the command-line tool (export, batch scripts)
  README.txt               this file
  RELEASE-NOTES.txt        what this version does and what is new
  KNOWN-ISSUES.txt         what is known to be wrong, missing or untested
  TROUBLESHOOTING.txt      what to do when something goes wrong
  LICENSE.txt              the terms of use (PolyForm Noncommercial 1.0.0)
  THIRD-PARTY-NOTICES.txt  code in the program that was not written for it
  BUILD.txt                how and from what this build was made
  SHA256SUMS.txt           a checksum of every file in this folder


1. Check that your copy is genuine
----------------------------------

Pikslar is not signed (see "Windows warnings" below), so Windows cannot tell
you who made it. A checksum can: the person who gave you Pikslar should send
you the SHA-256 checksum of the zip file in a SEPARATE message (email, chat),
not inside the zip.

  1. Open the folder that holds the downloaded zip.
  2. Windows 11: right-click an empty area, choose "Open in Terminal".
     Windows 10: hold Shift, right-click an empty area, choose
     "Open PowerShell window here".
  3. Type this and press Enter:

       Get-FileHash .\Pikslar-0.9.0-beta.1-windows-x64.zip

  4. The long code shown must match the one you were sent, exactly.

If it does not match, do not open the program; tell the person who sent it.
SHA256SUMS.txt and BUILD.txt came inside the same zip, so on their own they
prove nothing about where the zip came from. Once the zip is checked, they
let you check each file after unpacking ("Get-FileHash .\pikslar.exe").


2. Unpack and run
-----------------

  1. Right-click the zip, choose "Extract All...", and extract it to a folder
     you can write to, such as your Desktop or Documents. Do not run the
     program from inside the zip.
  2. Open the extracted folder and double-click pikslar.exe.

Nothing is installed. The folder can be moved, and the program can run from a
read-only folder or from Downloads, because it keeps its own data elsewhere
(see "What Pikslar writes").


Windows warnings
----------------

SmartScreen. The first time, Windows may show "Windows protected your PC".
Windows shows it for new programs that are not signed. It does not mean
anything was found in Pikslar, and it does not prove Pikslar is safe; that
depends on where your copy came from. If (and only if) you checked the
checksum above, choose
"More info", then "Run anyway". This applies to this one program and
changes no Windows setting.

Smart App Control. On some Windows 11 computers Smart App Control is on. It
blocks unsigned programs and offers no "Run anyway". Pikslar cannot run on
such a computer. Never turn Smart App Control off for Pikslar: once off, it
cannot be turned back on without reinstalling Windows. Tell the person who
gave you Pikslar instead.

Antivirus. An antivirus program may warn about a new, unsigned program.
Please report it; never add an exclusion or exception for Pikslar.

Work or school computers may forbid running programs like this. If you are
not sure it is allowed, do not run it.


What Pikslar writes, and where
------------------------------

Pikslar writes your work only where you tell it to: projects (.piks) and
exports (PNG, GIF, APNG and the rest) go where you choose in the Save and
Export windows. Beside a project it may briefly write name.piks.pikslar-tmp
and name.piks.pikslar-bak while saving (both are removed when the save
ends), and it keeps name.piks.v1.bak or .v2.bak when you upgrade a project
made by an older Pikslar. Sprite-sheet exports may get a .json file beside
them.

Everything else is in one folder of your own:

  %LOCALAPPDATA%\Pikslar\

  settings.txt   your settings: interface size, folded panels, recent files,
                 changed shortcuts, Rewind and diagnostics on or off. Not
                 written until you change a setting.
  recovery\      a copy of unsaved work, made about 8 seconds after a change,
                 so a crash costs at most the last few seconds. Removed when
                 you save or close normally.
  rewind\        undo history kept after closing, only if you turn Rewind on
                 (View menu; it is off unless you turn it on). At most
                 256 MiB per project and 2 GiB in all.
  log\           the saving log: at most two files of 64 KiB each.
  crash\         a short note for each unexpected close, the newest five.
  brushes.pkb    your custom brushes, if you make any.
  plugins\       plugins you have installed, if any.
  plugin-approvals.txt
                 which installed plugins you approved in the install review,
                 one line each. A plugin folder copied in by hand does not
                 load until you review and install it.

The log and crash notes ("diagnostics") record times, the version, what
happened (save, open, export, recovery, Rewind), how long it took, a Windows
error code and a file's name, such as walk.piks. They never record your
artwork, anything in a document, layer or tag names, script text, folder
names or anything you type. They stay on your computer. To turn them off:
View, "Stop keeping crash and saving notes". To delete them: View, "Delete
crash and saving notes".

Pikslar writes nothing to the registry and nothing beside pikslar.exe.


No network access
-----------------

Pikslar has no account, collects nothing and never connects to the
internet or any network: neither program contains any network code. Nothing
leaves your computer unless you send it yourself.

If you share your screen, remember that the Open and Save windows can show
the names of your folders.


How to remove everything
------------------------

  1. Close Pikslar.
  2. Delete the Pikslar folder you extracted (and the zip).
  3. Press Windows+R, type %LOCALAPPDATA%\Pikslar and press Enter. If a
     folder opens, go up one level and delete the Pikslar folder. This
     removes settings, recovery copies, Rewind history, diagnostics, brushes
     and plugins.

Your own projects and exports stay where you saved them. Nothing else was
installed. Windows itself may remember the last folder you used in its Save
window, as it does for any program.


Command-line options
--------------------

Most people never need these. In a terminal in the Pikslar folder:

  pikslar.exe drawing.piks      open a project (or a PNG, or an Aseprite
                                .ase or .aseprite file, which is only read)
  pikslar.exe --version         print the version and exit, without a window
  pikslar.exe --no-diagnostics  keep no saving log or crash note this time
  pikslar.exe --no-settings     read and write no settings this time
                                (diagnostics are off too)
  pikslar.exe --settings-dir D  keep settings and diagnostics in folder D
  pikslar.exe --no-recovery     make no recovery copies this time: a crash
                                then loses all unsaved work
  pikslar.exe --scale N         force the interface size (1 to 4)
  pikslar-cli.exe --help        what the command-line tool can do

The version is also shown in the Keyboard shortcuts window (View menu), and
in pikslar.exe's Properties, Details tab.


Reporting a problem
-------------------

See TROUBLESHOOTING.txt, "Sending a report".
