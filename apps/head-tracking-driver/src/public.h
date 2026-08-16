/*++

Copyright (c) Microsoft Corporation.  All rights reserved.

    THIS CODE AND INFORMATION IS PROVIDED "AS IS" WITHOUT WARRANTY OF ANY
    KIND, EITHER EXPRESSED OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE
    IMPLIED WARRANTIES OF MERCHANTABILITY AND/OR FITNESS FOR A PARTICULAR
    PURPOSE.

ModuleName:

    public.h

Abstract:

    Contains definitions used both by driver and application


--*/

#pragma once

/* AirPods AACP service: 74ec2172-0bad-4d01-8f77-997b2be0722a */
DEFINE_GUID(BTHECHOSAMPLE_SVC_GUID, 0x74ec2172, 0x0bad, 0x4d01, 0x8f, 0x77, 0x99, 0x7b, 0x2b, 0xe0, 0x72, 0x2a);
extern __declspec(selectany) const PWSTR BthEchoSampleSvcName = L"SdaAirPodsAacp";

//
// Device interface exposed by our bth client device
//

/* 8d09ce09-58c6-4f67-95a7-9824b4d54cb3 */
DEFINE_GUID(BTHECHOSAMPLE_DEVICE_INTERFACE, 0x8d09ce09, 0x58c6, 0x4f67, 0x95, 0xa7, 0x98, 0x24, 0xb4, 0xd5, 0x4c, 0xb3);

#define IOCTL_SDA_QUERY_CHANNEL_INFO \
    CTL_CODE(FILE_DEVICE_BLUETOOTH, 0x800, METHOD_BUFFERED, FILE_READ_ACCESS)

typedef struct _SDA_CHANNEL_INFO {
    ULONG Size;
    ULONG NegotiatedMode;
    ULONG OutMtu;
    ULONG InMtu;
    ULONG DeviceIdSdpPublished;
    NTSTATUS DeviceIdSdpStatus;
    ULONGLONG LocalFeaturesMask;
    ULONGLONG LocalBthAddress;
} SDA_CHANNEL_INFO, *PSDA_CHANNEL_INFO;
