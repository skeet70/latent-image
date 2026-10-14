/* Independent shim probes. Never load/init real libusb: all invoked entry
 * pointers below are local synthetic functions. Compile from this repository.
 */
#include <assert.h>
#include <stdint.h>
#ifndef F02_PROVIDER
#define F02_PROVIDER "../../transport/native_usb.c"
#endif
#include F02_PROVIDER

static libusb_device *fake_devices[] = {(libusb_device *)(uintptr_t)1, (libusb_device *)(uintptr_t)2, NULL};
static struct libusb_endpoint_descriptor endpoints[] = {
    {.bEndpointAddress=0x81,.bmAttributes=2,.wMaxPacketSize=512,.bInterval=2},
    {.bEndpointAddress=0x02,.bmAttributes=2,.wMaxPacketSize=512,.bInterval=1},
    {.bEndpointAddress=0x83,.bmAttributes=3,.wMaxPacketSize=16,.bInterval=8}
};
static struct libusb_interface_descriptor alternate = {.bInterfaceNumber=0,.bAlternateSetting=0,.bNumEndpoints=3,.bInterfaceClass=255,.endpoint=endpoints};
static struct libusb_interface iface = {.altsetting=&alternate,.num_altsetting=1};
static struct libusb_config_descriptor config = {.bNumInterfaces=1,.bConfigurationValue=1,.interface=&iface};
static int port_calls, config_calls, claimed_calls;
static ssize_t fake_list(libusb_context *ctx,libusb_device ***out) { (void)ctx; *out=fake_devices; return 2; }
static void fake_free_list(libusb_device **list,int unref) { assert(list==fake_devices && unref==1); }
static void fake_close(libusb_device_handle *value) { assert(value==(libusb_device_handle *)(uintptr_t)1); }
static int fake_descriptor(libusb_device *device,struct libusb_device_descriptor *out) {
    memset(out,0,sizeof *out);
    out->idVendor=device==fake_devices[0] ? 0x1d6b : 0x04cb;
    out->idProduct=device==fake_devices[0] ? 2 : 0xff80;
    return 0;
}
static int fake_ports(libusb_device *device,uint8_t *ports,int cap) {
    assert(device==fake_devices[1] && cap>=2); ++port_calls; ports[0]=2;ports[1]=3; return 2;
}
static uint8_t fake_bus(libusb_device *device) { assert(device==fake_devices[1]); return 1; }
static int fake_config(libusb_device *device,struct libusb_config_descriptor **out) { assert(device==fake_devices[1]);++config_calls;*out=&config;return 0; }
static void fake_free_config(struct libusb_config_descriptor *value) { assert(value==&config); }
static int fake_driver(libusb_device_handle *value,int interface) { (void)value;assert(interface==0);return 1; }
static int fake_claim(libusb_device_handle *value,int interface) { (void)value;(void)interface;++claimed_calls;return 0; }
static int fake_partial(libusb_device_handle *value,unsigned char endpoint,unsigned char *bytes,int size,int *transferred,unsigned int timeout) {
    (void)value;assert(endpoint==0x81 && size==128 && timeout==100);bytes[0]=0xaa;bytes[1]=0xbb;bytes[2]=0xcc;*transferred=3;return LIBUSB_ERROR_TIMEOUT;
}
/* S01 jig-mode layout (E-JIG-USB-DESCRIPTORS): bus 5 port 1, OUT listed before IN. */
static struct libusb_endpoint_descriptor s01_endpoints[] = {
    {.bEndpointAddress=0x01,.bmAttributes=2,.wMaxPacketSize=512,.bInterval=1},
    {.bEndpointAddress=0x82,.bmAttributes=2,.wMaxPacketSize=512,.bInterval=0},
    {.bEndpointAddress=0x83,.bmAttributes=3,.wMaxPacketSize=8,.bInterval=4}
};
static struct libusb_interface_descriptor s01_alternate = {.bInterfaceNumber=0,.bAlternateSetting=0,.bNumEndpoints=3,.bInterfaceClass=0xff,.bInterfaceSubClass=0x21,.bInterfaceProtocol=0xff,.endpoint=s01_endpoints};
static struct libusb_interface s01_iface = {.altsetting=&s01_alternate,.num_altsetting=1};
static struct libusb_config_descriptor s01_config = {.bNumInterfaces=1,.bConfigurationValue=1,.interface=&s01_iface};
static int s01_ports(libusb_device *device,uint8_t *ports,int cap) { assert(device==fake_devices[1] && cap>=1); ports[0]=1; return 1; }
static uint8_t s01_bus(libusb_device *device) { assert(device==fake_devices[1]); return 5; }
static int s01_config_for(libusb_device *device,struct libusb_config_descriptor **out) { assert(device==fake_devices[1]); *out=&s01_config; return 0; }
static void s01_free_config(struct libusb_config_descriptor *value) { assert(value==&s01_config); }
int main(void) {
    context=(libusb_context *)(uintptr_t)1;
    p_get_device_list=fake_list;p_get_device_descriptor=fake_descriptor;
    p_get_port_numbers=fake_ports;p_get_bus_number=fake_bus;
    p_get_active_config_descriptor=fake_config;p_free_config_descriptor=fake_free_config;
    assert(fuji_usb_list()==2);
    assert(port_calls==1 && config_calls==1);
    assert(strcmp(fuji_usb_path(0),"")==0 && strcmp(fuji_usb_path(1),"001:2.3")==0);
    assert(fuji_usb_field(1,12)==1 && fuji_usb_field(1,13)==0x83 && fuji_usb_field(1,14)==3 && fuji_usb_field(1,15)==16 && fuji_usb_field(1,16)==8);
    assert(fuji_usb_field(1,37)==2 && fuji_usb_field(1,38)==1 && fuji_usb_field(1,39)==-108);
    handle=(libusb_device_handle *)(uintptr_t)1;
    p_kernel_driver_active=fake_driver;p_claim_interface=fake_claim;
    assert(fuji_usb_claim()<0 && claimed_calls==0);
    claimed=1;p_bulk_transfer=fake_partial;
    unsigned char bytes[128]={0};int transferred=-1;
    assert(fuji_usb_transfer(1,0x81,bytes,128,100,&transferred)==LIBUSB_ERROR_TIMEOUT);
    assert(transferred==3 && bytes[0]==0xaa && bytes[1]==0xbb && bytes[2]==0xcc);
    p_close=fake_close;
    fuji_usb_close();
    p_free_device_list=fake_free_list;
    p_get_port_numbers=s01_ports;p_get_bus_number=s01_bus;
    p_get_active_config_descriptor=s01_config_for;p_free_config_descriptor=s01_free_config;
    assert(fuji_usb_list()==2 && strcmp(fuji_usb_path(1),"005:1")==0);
    /* Every slot FujiAF.Native reads, as {slot, expected}. */
    static const int s01_slots[][2] = {
        {0,0x04cb},{1,0xff80},{2,1},{3,0},{4,0},{5,0xff},{6,0x21},{7,0xff},{8,0x82},{9,0x01},{10,512},{11,512},
        {12,1},{13,0x83},{14,3},{15,8},{16,4},{37,0},{38,1}};
    for (size_t i=0; i<sizeof s01_slots/sizeof s01_slots[0]; ++i) assert(fuji_usb_field(1,s01_slots[i][0])==s01_slots[i][1]);
    puts("F02 native synthetic root-hub/extra-endpoint/driver/partial-result and S01 jig-layout probes passed");
    return 0;
}
