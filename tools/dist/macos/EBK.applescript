-- EBK.app. Books opened with it or dropped on it: .ebk books are opened in the EPUB reader of this Mac (Books,
-- unless another is chosen for EPUB files); EPUB files and folders are converted to .ebk files next to them.
-- The work is done by the program ebk inside the app (Contents/Resources/ebk).

on localized(chinese, english)
	if (user locale of (system info)) starts with "zh" then return chinese
	return english
end say

on ebkCommand()
	-- the language of the user for the messages of the program, which runs without one
	return "LANG=" & quoted form of ((user locale of (system info)) & ".UTF-8") & " " & quoted form of (POSIX path of (path to resource "ebk"))
end program

on run
	display dialog localized("把 EPUB 文件（或装着 EPUB 的文件夹）拖到 EBK 上，就会在旁边生成 .ebk 文件。" & return & return & "双击 .ebk 文件，就会用这台 Mac 上的 EPUB 阅读器（\"图书\"或你选的其它阅读器）打开它。", "Drop EPUB files (or folders with EPUB files) on EBK to convert them to .ebk files next to them." & return & return & "A double click on an .ebk file opens it in the EPUB reader of this Mac (Books, or the one you chose).") buttons {"OK"} default button 1 with title "EBK"
end run

on open theItems
	set books to ""
	set others to ""
	repeat with anItem in theItems
		set itemPath to POSIX path of anItem
		-- text comparisons of AppleScript ignore case: .EBK as well
		if itemPath ends with ".ebk" then
			set books to books & " " & quoted form of itemPath
		else
			set others to others & " " & quoted form of itemPath
		end if
	end repeat
	if books is not "" then
		try
			do shell script ebkCommand() & " open" & books
		on error errorText
			display dialog errorText buttons {"OK"} default button 1 with title "EBK" with icon caution
		end try
	end if
	if others is not "" then
		try
			set report to do shell script ebkCommand() & others & " 2>&1 | tail -n 12"
		on error errorText
			set report to errorText
		end try
		display dialog report buttons {"OK"} default button 1 with title "EBK"
	end if
end open
