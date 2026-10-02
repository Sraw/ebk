#!/usr/bin/env python3
"""Usage: info_plist.py <EBK.app/Contents/Info.plist> <version>

What osacompile writes, plus: the file type .ebk (declared here, and owned by the app, so that a double click
on a book opens it with EBK.app) and EPUB files and folders as files the app takes too, without becoming the
program for them.
"""
import plistlib, sys

ID = "io.github.sraw.ebk"  # the identifier of the app; the type of .ebk files is ID + ".book"

path, version = sys.argv[1:3]
with open(path, "rb") as f:
    info = plistlib.load(f)
info.update({
    "CFBundleIdentifier": ID,
    "CFBundleName": "EBK",
    "CFBundleShortVersionString": version,
    "CFBundleVersion": version,
    "LSMinimumSystemVersion": "11.0",
    "UTExportedTypeDeclarations": [{
        "UTTypeIdentifier": ID + ".book",
        "UTTypeDescription": "EBK book",
        "UTTypeConformsTo": ["public.data", "public.content"],
        "UTTypeTagSpecification": {"public.filename-extension": ["ebk"], "public.mime-type": ["application/x-ebk"]},
    }],
    "CFBundleDocumentTypes": [
        {"CFBundleTypeName": "EBK book", "CFBundleTypeRole": "Viewer", "LSHandlerRank": "Owner", "LSItemContentTypes": [ID + ".book"]},
        {"CFBundleTypeName": "EPUB book", "CFBundleTypeRole": "Viewer", "LSHandlerRank": "Alternate", "LSItemContentTypes": ["org.idpf.epub-container"]},
        {"CFBundleTypeName": "Folder", "CFBundleTypeRole": "Viewer", "LSHandlerRank": "Alternate", "LSItemContentTypes": ["public.folder"]},
    ],
})
with open(path, "wb") as f:
    plistlib.dump(info, f)
