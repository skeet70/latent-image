/* Narrow opt-in provider with no linked libusb dependency and no constructor.
 * The runtime path below is the locked Nix libusb, fixed at build time; it is
 * opened only from the live camera operations.
 */
#include <libusb.h>
#ifndef FUJI_LIBUSB_PATH
#include "transport/libusb_path.h"
#endif
#include <dlfcn.h>
#include <stdio.h>
#include <string.h>
#include <sys/vfs.h>
#include <linux/magic.h>

static void *library;
static libusb_context *context;
static libusb_device **devices;
static libusb_device_handle *handle;
static int claimed;
static ssize_t count;
static char paths[64][80];
static int fields[64][39];

#define APIS(X) \
 X(init_context) X(exit) X(get_device_list) X(free_device_list) \
 X(get_device_descriptor) X(get_bus_number) X(get_port_numbers) \
 X(get_active_config_descriptor) X(free_config_descriptor) X(open) \
 X(kernel_driver_active) X(claim_interface) X(release_interface) X(close) \
 X(control_transfer) X(bulk_transfer)
#define DECLARE(name) static __typeof__(&libusb_##name) p_##name;
APIS(DECLARE)

int fuji_usb_load(void) {
    struct statfs fs;
    if (library || statfs("/sys/bus/usb/devices", &fs) != 0 || fs.f_type != SYSFS_MAGIC) return -100;
    /* Reject fallback USBFS enumeration, whose descriptor acquisition can open
     * unselected nodes and issue GET_CONFIGURATION. No fallback is supported. */
    library = dlopen(FUJI_LIBUSB_PATH, RTLD_NOW | RTLD_LOCAL);
    if (!library) return -101;
#define LOAD(name) do { p_##name = (__typeof__(p_##name))dlsym(library, "libusb_" #name); if (!p_##name) return -102; } while (0);
    APIS(LOAD)
    return 0;
}

int fuji_usb_init(void) {
    if (!library || context) return -103;
    const struct libusb_init_option options[] = {{.option = LIBUSB_OPTION_LOG_LEVEL, .value.ival = LIBUSB_LOG_LEVEL_NONE}};
    return p_init_context(&context, options, 1);
}

/* Re-listing is allowed only while no device is open: fuji_usb_open resolves
 * its path against this list, and a handle holds its own reference. */
int fuji_usb_list(void) {
    if (!context || handle) return -104;
    if (devices) { p_free_device_list(devices,1); devices=NULL; count=0; }
    count = p_get_device_list(context, &devices);
    if (count < 0) return (int)count;
    if (count > 64) return -105;
    for (int i = 0; i < count; ++i) {
        struct libusb_device_descriptor dd;
        int result = p_get_device_descriptor(devices[i], &dd);
        if (result) return result;
        fields[i][0] = dd.idVendor; fields[i][1] = dd.idProduct;
        if (dd.idVendor != 0x04cb || dd.idProduct != 0xff80) { paths[i][0]='\0'; continue; }
        unsigned char ports[8];
        result = p_get_port_numbers(devices[i], ports, 8);
        if (result <= 0 || result > 8) return -106;
        int used = snprintf(paths[i], sizeof paths[i], "%03u:", p_get_bus_number(devices[i]));
        for (int j = 0; j < result; ++j) used += snprintf(paths[i]+used, sizeof paths[i]-(size_t)used, "%s%u", j ? "." : "", ports[j]);
        /* Descriptor expectations apply only to eligible devices. Other USB
         * devices are never opened, claimed or interrogated through a handle. */
        struct libusb_config_descriptor *config = NULL;
        result = p_get_active_config_descriptor(devices[i], &config);
        if (result) return result;
        int valid = config->bNumInterfaces == 1 && config->interface[0].num_altsetting == 1;
        if (valid) {
            const struct libusb_interface_descriptor *iface = &config->interface[0].altsetting[0];
            valid = iface->bInterfaceNumber == 0 && iface->bAlternateSetting == 0 && iface->bNumEndpoints >= 2 && iface->bNumEndpoints <= 8;
            fields[i][2] = config->bConfigurationValue;
            fields[i][3] = iface->bInterfaceNumber; fields[i][4] = iface->bAlternateSetting;
            fields[i][5] = iface->bInterfaceClass; fields[i][6] = iface->bInterfaceSubClass; fields[i][7] = iface->bInterfaceProtocol;
            int ins=0,outs=0,extras=0;
            for (int j=0; valid && j<iface->bNumEndpoints; ++j) {
                const struct libusb_endpoint_descriptor *ep = &iface->endpoint[j];
                if ((ep->bmAttributes & LIBUSB_TRANSFER_TYPE_MASK) != LIBUSB_TRANSFER_TYPE_BULK) {
                    if (extras>=6) { valid=0; break; }
                    fields[i][13+extras*4]=ep->bEndpointAddress; fields[i][14+extras*4]=ep->bmAttributes;
                    fields[i][15+extras*4]=ep->wMaxPacketSize; fields[i][16+extras*4]=ep->bInterval;
                    ++extras; continue;
                }
                if (ep->bmAttributes != LIBUSB_TRANSFER_TYPE_BULK) { valid=0; break; }
                if (ep->bEndpointAddress & LIBUSB_ENDPOINT_IN) { ++ins; fields[i][8]=ep->bEndpointAddress; fields[i][10]=ep->wMaxPacketSize; fields[i][37]=ep->bInterval; }
                else { ++outs; fields[i][9]=ep->bEndpointAddress; fields[i][11]=ep->wMaxPacketSize; fields[i][38]=ep->bInterval; }
            }
            valid = valid && ins == 1 && outs == 1;
            fields[i][12]=extras;
        }
        p_free_config_descriptor(config);
        if (!valid) return -107;
    }
    return (int)count;
}
const char *fuji_usb_path(int i) { return i>=0 && i<count && i<64 ? paths[i] : NULL; }
int fuji_usb_field(int i, int j) { return i>=0 && i<count && i<64 && j>=0 && j<39 ? fields[i][j] : -108; }
int fuji_usb_open(const char *path) {
    if (!devices || handle) return -109;
    int found=-1;
    for (int i=0;i<count;++i) if (!strcmp(paths[i],path)) { if(found>=0) return -110; found=i; }
    if(found<0 || fields[found][0]!=0x04cb || fields[found][1]!=0xff80) return -111;
    return p_open(devices[found], &handle);
}
int fuji_usb_claim(void) {
    if (!handle || claimed) return -112;
    int result = p_kernel_driver_active(handle,0);
    if (result != 0) return result < 0 ? result : -113;
    result = p_claim_interface(handle,0);
    if (!result) claimed=1;
    return result;
}
/* Returns API status separately from transferred count, including partial errors.
   33000 ms is transferCeilingMs, the held commit-f7 trigger reply. A bulk IN
   (kind 2) admits 0x100000 bytes, one read-bulk chunk; every other kind 292. */
int fuji_usb_transfer(int kind,int endpoint,unsigned char *bytes,int size,unsigned int timeout,int *transferred) {
    *transferred=0;
    if (!handle || !claimed || timeout==0 || timeout>33000 || size<1 || size>(kind==2 ? 0x100000 : 292)) return -114;
    if (kind==0) {
        if (size!=12) return -115;
        int result=p_control_transfer(handle,0x43,0,0,0,bytes,(uint16_t)size,timeout);
        if(result<0) return result;
        *transferred=result; return 0;
    }
    if (kind!=1 && kind!=2) return -116;
    return p_bulk_transfer(handle,(unsigned char)endpoint,bytes,size,transferred,timeout);
}
int fuji_usb_release(void) { if(!handle || !claimed) return 0; int result=p_release_interface(handle,0); if(!result) claimed=0; return result; }
int fuji_usb_close(void) { if(handle) { p_close(handle); handle=NULL; claimed=0; } return 0; }
int fuji_usb_exit(void) {
    if(devices) { p_free_device_list(devices,1); devices=NULL; count=0; }
    if(context) { p_exit(context); context=NULL; }
    if(library) { dlclose(library); library=NULL; }
    return 0;
}
