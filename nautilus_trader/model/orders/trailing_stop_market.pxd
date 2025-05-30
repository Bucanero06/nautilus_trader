# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from libc.stdint cimport uint64_t

from nautilus_trader.core.rust.model cimport TrailingOffsetType
from nautilus_trader.core.rust.model cimport TriggerType
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orders.base cimport Order


cdef class TrailingStopMarketOrder(Order):
    cdef readonly Price activation_price
    """The order activation price (STOP).\n\n:returns: `Price` or ``None``"""
    cdef readonly Price trigger_price
    """The order trigger price (STOP).\n\n:returns: `Price` or ``None``"""
    cdef readonly TriggerType trigger_type
    """The trigger type for the order.\n\n:returns: `TriggerType`"""
    cdef readonly object trailing_offset
    """The trailing offset for the orders trigger price (STOP).\n\n:returns: `Decimal`"""
    cdef readonly TrailingOffsetType trailing_offset_type
    """The trailing offset type.\n\n:returns: `TrailingOffsetType`"""
    cdef readonly uint64_t expire_time_ns
    """The order expiration (UNIX epoch nanoseconds), zero for no expiration.\n\n:returns: `uint64_t`"""
    cdef readonly bint is_activated
    """If the order has been activated.\n\n:returns: `bool`"""

    @staticmethod
    cdef TrailingStopMarketOrder create_c(OrderInitialized init)
